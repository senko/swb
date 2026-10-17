//! The matcher: runs a [`Program`] over an input as a loop with an
//! explicit backtrack stack and an undo log, without Rust recursion (ADR
//! 0026 section 10; ECMA-262 §22.2.2 and the start-position loop of
//! §22.2.7.2, `RegExpBuiltinExec`).
//!
//! A choice point records the program position and the input position to
//! go back to, and the length of the undo log. Every change of a slot (a
//! capture, a loop counter, a recorded position) first logs the old value,
//! unless the slot was already logged since the last choice point (each
//! choice point has a unique generation number); backtracking restores the
//! log down to the choice point.
//!
//! The matcher stops after a given number of steps and continues on the
//! next call: all its state is in [`Matcher`], and positions are code-unit
//! indices, so the caller can run its time check and a garbage collection
//! between two calls and pass the input again.

use swb_js_text::{CodeUnit, Str16, is_lead_surrogate, is_trail_surrogate};

use crate::ast::{Assertion, INFINITE};
use crate::charset::{self, CaseMode};
use crate::error::Error;
use crate::program::{Inst, Prefix, Program, Test, UNSET};
use crate::{Captures, Outcome, Regex};

/// A choice point.
#[derive(Clone, Copy, Debug)]
enum Choice {
    /// Continue at `pc` with `pos`.
    Resume { pc: u32, pos: u32 },
    /// A greedy loop of one-character tests at `pos`: give back one
    /// character (towards `min_pos`) and continue at `pc`.
    GiveBack {
        pc: u32,
        pos: u32,
        min_pos: u32,
        back: bool,
    },
    /// A lazy loop of one-character tests at instruction `pc`, after
    /// `count` characters: match one more.
    LazyMore { pc: u32, pos: u32, count: u32 },
    /// The start of a lookaround at `pos`; `cont` is where a negative
    /// lookaround continues when its body fails.
    Look { pos: u32, negate: bool, cont: u32 },
}

/// A choice point with its undo mark and generation.
#[derive(Clone, Copy, Debug)]
struct Frame {
    choice: Choice,
    undo: u32,
    generation: u64,
}

/// Work that an instruction did not finish before the steps ran out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Partial {
    /// A loop of one-character tests: the count, the position and the
    /// position after the minimum.
    Scan { count: u32, pos: u32, min_pos: u32 },
    /// A backreference: the code units compared so far.
    Compare { done: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Before the first call.
    New,
    /// Looking for the next start position.
    Search,
    /// Running an attempt at `start`.
    Running,
    /// The search ended.
    Done,
}

/// The state of a match of one [`Regex`] on one input, from a start
/// position. Create it with [`Matcher::new`] and call [`Matcher::run`]
/// until it returns something other than [`Outcome::Pending`].
#[derive(Debug)]
pub struct Matcher {
    phase: Phase,
    /// The start of the current attempt.
    start: u32,
    pc: u32,
    pos: u32,
    slots: Vec<u32>,
    logged: Vec<u64>,
    stack: Vec<Frame>,
    undo: Vec<(u32, u32)>,
    generation: u64,
    partial: Option<Partial>,
    input_len: usize,
    steps: u64,
    /// The memory limit of the current call.
    memory_limit: usize,
    /// The undo log would grow beyond the memory limit.
    over_limit: bool,
    /// A backtrack was interrupted by the end of the step budget and
    /// continues in the next call.
    backtracking: bool,
    /// The bytes of the slots and their undo markers, which the memory
    /// limit also covers.
    fixed_bytes: usize,
}

/// What the attempt does after an instruction.
enum Flow {
    Continue,
    Fail,
}

/// The result of one instruction.
enum Step {
    /// Go on at the new program position.
    Next,
    /// Backtrack.
    Fail,
    /// The pattern matched.
    Matched,
    /// The steps ran out inside the instruction.
    Pending,
    /// An internal error ends the attempt.
    Abort,
}

/// Reads characters from the input in both directions.
struct Input<'a, U> {
    units: &'a [U],
    unicode: bool,
}

impl<U: CodeUnit> Input<'_, U> {
    #[inline]
    fn unit(&self, i: usize) -> Option<u16> {
        self.units.get(i).map(|u| u.value())
    }

    /// The character at `pos` and the position after it.
    #[inline]
    fn next(&self, pos: usize) -> Option<(u32, usize)> {
        let unit = self.unit(pos)?;
        if self.unicode
            && is_lead_surrogate(unit)
            && let Some(trail) = self.unit(pos + 1)
            && is_trail_surrogate(trail)
        {
            return Some((swb_js_text::combine_surrogates(unit, trail), pos + 2));
        }
        Some((u32::from(unit), pos + 1))
    }

    /// The character before `pos` and the position where it starts.
    #[inline]
    fn prev(&self, pos: usize) -> Option<(u32, usize)> {
        let unit = self.unit(pos.checked_sub(1)?)?;
        if self.unicode
            && is_trail_surrogate(unit)
            && let Some(lead) = pos.checked_sub(2).and_then(|i| self.unit(i))
            && is_lead_surrogate(lead)
        {
            return Some((swb_js_text::combine_surrogates(lead, unit), pos - 2));
        }
        Some((u32::from(unit), pos - 1))
    }

    #[inline]
    fn read(&self, pos: usize, back: bool) -> Option<(u32, usize)> {
        if back { self.prev(pos) } else { self.next(pos) }
    }

    fn is_word(&self, pos: Option<usize>, extra: bool) -> bool {
        let Some(c) = pos.and_then(|p| self.unit(p)) else {
            return false;
        };
        let c = u32::from(c);
        c < 0x80 && ((c as u8).is_ascii_alphanumeric() || c == 0x5F)
            || (extra && (c == 0x17F || c == 0x212A))
    }
}

fn is_line_terminator(c: u32) -> bool {
    matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029)
}

#[inline]
fn test_char(program: &Program, test: Test, c: u32) -> bool {
    match test {
        Test::Char(x) => c == x,
        Test::Set(i) => program.sets.get(i as usize).is_some_and(|s| s.contains(c)),
        Test::Any => true,
        Test::AnyButLineTerminator => !is_line_terminator(c),
    }
}

impl Matcher {
    /// A match of `regex` that starts its search at code unit `start`
    /// (`lastIndex`). With the `y` flag it tries only that position.
    pub fn new(regex: &Regex, start: usize) -> Matcher {
        let program = &regex.program;
        Matcher {
            phase: Phase::New,
            start: u32::try_from(start).unwrap_or(u32::MAX),
            pc: 0,
            pos: 0,
            slots: vec![UNSET; program.slot_count as usize],
            logged: vec![0; program.slot_count as usize],
            stack: Vec::new(),
            undo: Vec::new(),
            generation: 0,
            partial: None,
            input_len: 0,
            steps: 0,
            memory_limit: 0,
            over_limit: false,
            backtracking: false,
            fixed_bytes: program.slot_count as usize * (size_of::<u32>() + size_of::<u64>()),
        }
    }

    /// Makes the state that of [`Matcher::new`]`(regex, start)` and keeps
    /// the vectors, so that a loop over the matches of one input (a global
    /// `replace`) allocates them once. The generation counter keeps
    /// growing, so the marks of the undo log stay valid.
    pub fn reset(&mut self, regex: &Regex, start: usize) {
        let slot_count = regex.program.slot_count as usize;
        self.slots.clear();
        self.slots.resize(slot_count, UNSET);
        if self.logged.len() != slot_count {
            self.logged.clear();
            self.logged.resize(slot_count, 0);
        }
        self.phase = Phase::New;
        self.start = u32::try_from(start).unwrap_or(u32::MAX);
        self.pc = 0;
        self.pos = 0;
        self.stack.clear();
        self.undo.clear();
        self.partial = None;
        self.input_len = 0;
        self.steps = 0;
        self.memory_limit = 0;
        self.over_limit = false;
        self.backtracking = false;
        self.fixed_bytes = slot_count * (size_of::<u32>() + size_of::<u64>());
    }

    /// The bytes that the state uses on the heap (the backtrack stack, the
    /// undo log and the slots), for the caller's heap accounting.
    pub fn heap_bytes(&self) -> usize {
        self.stack.capacity() * size_of::<Frame>()
            + self.undo.capacity() * size_of::<(u32, u32)>()
            + self.slots.capacity() * 4
            + self.logged.capacity() * 8
    }

    /// The number of steps run so far.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Runs the match for at most `steps` steps (instructions, characters
    /// of a loop, choice points restored). `input` must be the same text
    /// on every call. `memory_limit` bounds the bytes of the backtrack
    /// stack and the undo log ([`Error::MemoryLimit`] beyond it).
    pub fn run(
        &mut self,
        regex: &Regex,
        input: Str16<'_>,
        steps: u32,
        memory_limit: usize,
    ) -> Result<Outcome, Error> {
        if self.phase == Phase::New {
            self.input_len = input.len();
        } else if input.len() != self.input_len {
            return Err(Error::InputChanged);
        }
        if input.len() >= u32::MAX as usize {
            return Err(Error::MemoryLimit);
        }
        self.memory_limit = memory_limit;
        if self.fixed_bytes > memory_limit {
            return Err(Error::MemoryLimit);
        }
        let mut budget = steps;
        let result = match input {
            Str16::Latin1(units) => self.drive(regex, units, &mut budget, memory_limit),
            Str16::Wide(units) => self.drive(regex, units, &mut budget, memory_limit),
        };
        self.steps += u64::from(steps - budget);
        result
    }

    fn drive<U: CodeUnit>(
        &mut self,
        regex: &Regex,
        units: &[U],
        budget: &mut u32,
        memory_limit: usize,
    ) -> Result<Outcome, Error> {
        let program = &regex.program;
        let input = Input {
            units,
            unicode: program.unicode,
        };
        let len = units.len() as u32;
        loop {
            match self.phase {
                Phase::New => {
                    if self.start > len {
                        self.phase = Phase::Done;
                        return Ok(Outcome::NoMatch);
                    }
                    // A start inside a surrogate pair starts at the pair
                    // (§22.2.7.2: the character that contains lastIndex).
                    let s = self.start as usize;
                    if program.unicode
                        && s > 0
                        && input.unit(s).is_some_and(is_trail_surrogate)
                        && input.unit(s - 1).is_some_and(is_lead_surrogate)
                    {
                        self.start -= 1;
                    }
                    self.phase = Phase::Search;
                }
                Phase::Search => {
                    if !self.find_start(program, &input, budget) {
                        if self.phase == Phase::Done {
                            return Ok(Outcome::NoMatch);
                        }
                        return Ok(Outcome::Pending);
                    }
                    self.begin_attempt(program, budget);
                    self.phase = Phase::Running;
                }
                Phase::Running => match self.execute(program, &input, budget, memory_limit)? {
                    Some(true) => {
                        self.phase = Phase::Done;
                        return Ok(Outcome::Match(self.captures(program)));
                    }
                    Some(false) => {
                        if program.sticky || self.start >= len {
                            self.phase = Phase::Done;
                            return Ok(Outcome::NoMatch);
                        }
                        self.start = input
                            .next(self.start as usize)
                            .map_or(len + 1, |(_, n)| n as u32);
                        self.phase = Phase::Search;
                    }
                    None => return Ok(Outcome::Pending),
                },
                Phase::Done => return Ok(Outcome::NoMatch),
            }
        }
    }

    /// Moves `start` to the next position where a match can start. Returns
    /// false if the steps ran out (or the search ended: `Done`).
    fn find_start<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        budget: &mut u32,
    ) -> bool {
        let len = input.units.len() as u32;
        let Some(prefix) = &program.prefix else {
            return true;
        };
        loop {
            if self.start >= len {
                self.phase = Phase::Done;
                return false;
            }
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let Some((c, next)) = input.next(self.start as usize) else {
                self.phase = Phase::Done;
                return false;
            };
            let found = match prefix {
                Prefix::Char(x) => c == *x,
                Prefix::Set(set) => set.contains(c),
            };
            if found {
                return true;
            }
            self.start = next as u32;
        }
    }

    fn begin_attempt(&mut self, program: &Program, budget: &mut u32) {
        let captures = ((program.group_count + 1) * 2) as usize;
        for slot in self.slots.iter_mut().take(captures) {
            *slot = UNSET;
        }
        *budget = budget.saturating_sub((captures / 32) as u32);
        self.stack.clear();
        self.undo.clear();
        self.partial = None;
        self.pc = 0;
        self.pos = self.start;
    }

    fn captures(&self, program: &Program) -> Captures {
        let mut spans = Vec::with_capacity(program.group_count as usize + 1);
        spans.push(Some((self.start as usize, self.pos as usize)));
        for g in 1..=program.group_count {
            let s = Program::capture_slot(g) as usize;
            let (start, end) = (self.slots.get(s), self.slots.get(s + 1));
            spans.push(match (start, end) {
                (Some(&a), Some(&b)) if a != UNSET && b != UNSET => Some((a as usize, b as usize)),
                _ => None,
            });
        }
        Captures { spans }
    }

    #[inline]
    fn read_slot(&self, slot: u32) -> u32 {
        self.slots.get(slot as usize).copied().unwrap_or(UNSET)
    }

    /// Sets a slot and logs its old value for backtracking.
    #[inline]
    fn write(&mut self, slot: u32, value: u32) {
        let top = self.stack.last().map_or(0, |f| f.generation);
        let i = slot as usize;
        let (Some(current), Some(logged)) = (self.slots.get(i).copied(), self.logged.get_mut(i))
        else {
            return;
        };
        if current == value {
            return;
        }
        if *logged != top {
            *logged = top;
            // A full vector doubles: check the size before the growth.
            if self.undo.len() == self.undo.capacity()
                && (self.undo.capacity() * 2).max(8) * size_of::<(u32, u32)>()
                    + self.stack.capacity() * size_of::<Frame>()
                    + self.fixed_bytes
                    > self.memory_limit
            {
                self.over_limit = true;
                return;
            }
            self.undo.push((slot, current));
        }
        self.slots[i] = value;
    }

    fn push(&mut self, choice: Choice, memory_limit: usize) -> Result<(), Error> {
        self.generation += 1;
        let frame = Frame {
            choice,
            undo: self.undo.len() as u32,
            generation: self.generation,
        };
        // A full vector doubles: check the size after the growth.
        if self.stack.len() == self.stack.capacity() {
            let bytes = (self.stack.capacity() * 2).max(8) * size_of::<Frame>()
                + self.undo.capacity() * size_of::<(u32, u32)>()
                + self.fixed_bytes;
            if bytes > memory_limit {
                return Err(Error::MemoryLimit);
            }
        }
        self.stack.push(frame);
        Ok(())
    }

    fn restore(&mut self, mark: u32) {
        while self.undo.len() > mark as usize {
            if let Some((slot, old)) = self.undo.pop()
                && let Some(s) = self.slots.get_mut(slot as usize)
            {
                *s = old;
            }
        }
    }

    /// Backtracks to the newest choice point that continues the attempt.
    /// Returns false if there is none: the attempt failed. Returns true
    /// with an empty budget if the steps ran out while popping choice
    /// points: the next call goes on (`backtracking`).
    fn backtrack<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        budget: &mut u32,
        memory_limit: usize,
    ) -> Result<bool, Error> {
        self.partial = None;
        self.backtracking = false;
        while let Some(frame) = self.stack.pop() {
            if *budget == 0 {
                // Every popped frame costs a step: put it back and go on in
                // the next call. The caller sees `budget == 0` and returns.
                self.stack.push(frame);
                self.backtracking = true;
                return Ok(true);
            }
            *budget -= 1;
            self.restore(frame.undo);
            match frame.choice {
                Choice::Resume { pc, pos } => {
                    self.pc = pc;
                    self.pos = pos;
                    return Ok(true);
                }
                Choice::GiveBack {
                    pc,
                    pos,
                    min_pos,
                    back,
                } => {
                    let Some((_, p)) = input.read(pos as usize, !back) else {
                        continue;
                    };
                    let p = p as u32;
                    if p != min_pos {
                        self.push(
                            Choice::GiveBack {
                                pc,
                                pos: p,
                                min_pos,
                                back,
                            },
                            memory_limit,
                        )?;
                    }
                    self.pc = pc;
                    self.pos = p;
                    return Ok(true);
                }
                Choice::LazyMore { pc, pos, count } => {
                    let Some(&Inst::Repeat {
                        test, back, max, ..
                    }) = program.insts.get(pc as usize)
                    else {
                        continue;
                    };
                    let Some((c, p)) = input.read(pos as usize, back) else {
                        continue;
                    };
                    if !test_char(program, test, c) {
                        continue;
                    }
                    let count = count + 1;
                    if max == INFINITE || count < max {
                        self.push(
                            Choice::LazyMore {
                                pc,
                                pos: p as u32,
                                count,
                            },
                            memory_limit,
                        )?;
                    }
                    self.pc = pc + 1;
                    self.pos = p as u32;
                    return Ok(true);
                }
                Choice::Look { pos, negate, cont } => {
                    if negate {
                        self.pc = cont;
                        self.pos = pos;
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Runs the attempt. `Some(true)`: matched; `Some(false)`: failed;
    /// `None`: out of steps.
    fn execute<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        budget: &mut u32,
        memory_limit: usize,
    ) -> Result<Option<bool>, Error> {
        if self.backtracking {
            // The last call ran out of steps while popping choice points.
            if !self.backtrack(program, input, budget, memory_limit)? {
                return Ok(Some(false));
            }
        }
        loop {
            if *budget == 0 {
                return Ok(None);
            }
            *budget -= 1;
            let Some(&inst) = program.insts.get(self.pc as usize) else {
                return Ok(Some(false));
            };
            let flow = match self.step(program, input, inst, budget, memory_limit)? {
                Step::Next => Flow::Continue,
                Step::Fail => Flow::Fail,
                Step::Matched => return Ok(Some(true)),
                Step::Pending => return Ok(None),
                Step::Abort => return Ok(Some(false)),
            };
            if self.over_limit {
                return Err(Error::MemoryLimit);
            }
            if let Flow::Fail = flow
                && !self.backtrack(program, input, budget, memory_limit)?
            {
                return Ok(Some(false));
            }
        }
    }

    /// `Inst::Switch`: follows the edge of a trie node for the next
    /// character.
    fn switch<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        table: u32,
        back: bool,
        fold: bool,
    ) -> Step {
        let target = input.read(self.pos as usize, back).and_then(|(c, p)| {
            let c = if fold {
                charset::canonicalize(CaseMode::Fold, c)
            } else {
                c
            };
            let entries = program.tables.get(table as usize)?;
            let i = entries.binary_search_by_key(&c, |e| e.0).ok()?;
            Some((entries.get(i)?.1, p))
        });
        match target {
            Some((pc, p)) => {
                self.pc = pc;
                self.pos = p as u32;
                Step::Next
            }
            None => Step::Fail,
        }
    }

    /// Runs one instruction.
    fn step<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        inst: Inst,
        budget: &mut u32,
        memory_limit: usize,
    ) -> Result<Step, Error> {
        let next = |m: &mut Self| {
            m.pc += 1;
            Step::Next
        };
        Ok(match inst {
            Inst::Match => Step::Matched,
            Inst::Test { test, back } => match input.read(self.pos as usize, back) {
                Some((c, p)) if test_char(program, test, c) => {
                    self.pos = p as u32;
                    next(self)
                }
                _ => Step::Fail,
            },
            Inst::Repeat {
                test,
                back,
                greedy,
                min,
                max,
            } => self.repeat(
                program,
                input,
                (test, back, greedy, min, max),
                budget,
                memory_limit,
            )?,
            Inst::Switch { table, back, fold } => self.switch(program, input, table, back, fold),
            Inst::Assert(a) if self.assert(input, a) => next(self),
            Inst::Assert(_) => Step::Fail,
            Inst::Fork { alt } => {
                self.push(
                    Choice::Resume {
                        pc: alt,
                        pos: self.pos,
                    },
                    memory_limit,
                )?;
                next(self)
            }
            Inst::ForkFirst { first } => {
                let pc = self.pc + 1;
                self.push(Choice::Resume { pc, pos: self.pos }, memory_limit)?;
                self.pc = first;
                Step::Next
            }
            Inst::Jump { to } => {
                self.pc = to;
                Step::Next
            }
            Inst::Backref {
                list,
                back,
                ignore_case,
            } => match self.backref(program, input, list, back, ignore_case, budget) {
                Some(true) => next(self),
                Some(false) => Step::Fail,
                None => Step::Pending,
            },
            Inst::LoopHead {
                reg,
                min,
                max,
                greedy,
                exit,
            } => {
                self.loop_head(reg, min, max, greedy, exit, memory_limit)?;
                Step::Next
            }
            Inst::LookStart { reg, negate, end } => {
                let depth = self.stack.len() as u32;
                let look = Choice::Look {
                    pos: self.pos,
                    negate,
                    cont: end,
                };
                self.push(look, memory_limit)?;
                if let Some(s) = self.slots.get_mut(reg as usize) {
                    *s = depth;
                }
                next(self)
            }
            Inst::LookEnd { reg, negate } => self.look_end(reg, negate),
            Inst::Open { .. }
            | Inst::Close { .. }
            | Inst::Reset { .. }
            | Inst::LoopInit { .. }
            | Inst::LoopTail { .. }
            | Inst::SavePos { .. }
            | Inst::CheckProgress { .. } => self.slot_step(program, inst, budget),
        })
    }

    /// The instructions that only read and write slots.
    fn slot_step(&mut self, program: &Program, inst: Inst, budget: &mut u32) -> Step {
        let next = |m: &mut Self| {
            m.pc += 1;
            Step::Next
        };
        match inst {
            Inst::Open { group } => {
                self.write(program.open_slot(group), self.pos);
                next(self)
            }
            Inst::Close { group } => {
                let open = self.read_slot(program.open_slot(group));
                let s = Program::capture_slot(group);
                self.write(s, open.min(self.pos));
                self.write(s + 1, open.max(self.pos));
                next(self)
            }
            Inst::Reset { first, count } => {
                *budget = budget.saturating_sub(count / 8);
                for g in first + 1..=first + count {
                    let s = Program::capture_slot(g);
                    self.write(s, UNSET);
                    self.write(s + 1, UNSET);
                }
                next(self)
            }
            Inst::LoopInit { reg } => {
                self.write(reg, 0);
                next(self)
            }
            Inst::LoopTail {
                reg,
                min,
                head,
                empty_check,
            } => {
                let count = self.read_slot(reg);
                if empty_check && count >= min && self.pos == self.read_slot(reg + 1) {
                    Step::Fail
                } else {
                    self.write(reg, count.saturating_add(1));
                    self.pc = head;
                    Step::Next
                }
            }
            Inst::SavePos { reg } => {
                self.write(reg, self.pos);
                next(self)
            }
            Inst::CheckProgress { reg } if self.pos == self.read_slot(reg) => Step::Fail,
            Inst::CheckProgress { .. } => next(self),
            _ => Step::Abort,
        }
    }

    /// A loop of one-character tests (`Inst::Repeat`): matches as many
    /// characters as allowed (greedy) or the minimum (lazy), and leaves a
    /// choice point that gives back or takes one more.
    fn repeat<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        (test, back, greedy, min, max): (Test, bool, bool, u32, u32),
        budget: &mut u32,
        memory_limit: usize,
    ) -> Result<Step, Error> {
        let (mut count, mut pos, mut min_pos) = match self.partial.take() {
            Some(Partial::Scan {
                count,
                pos,
                min_pos,
            }) => (count, pos, min_pos),
            _ => (0, self.pos, self.pos),
        };
        let limit = if greedy { max } else { min };
        let mut first = true;
        while count < limit && count < INFINITE - 1 {
            // At least one character per call, so that a resumed loop
            // always moves on.
            if *budget == 0 && !first {
                self.partial = Some(Partial::Scan {
                    count,
                    pos,
                    min_pos,
                });
                return Ok(Step::Pending);
            }
            *budget = budget.saturating_sub(1);
            first = false;
            match input.read(pos as usize, back) {
                Some((c, p)) if test_char(program, test, c) => {
                    pos = p as u32;
                    count += 1;
                    if count == min {
                        min_pos = pos;
                    }
                }
                _ => break,
            }
        }
        if count < min {
            return Ok(Step::Fail);
        }
        if greedy && count > min {
            let pc = self.pc + 1;
            let choice = Choice::GiveBack {
                pc,
                pos,
                min_pos,
                back,
            };
            self.push(choice, memory_limit)?;
        } else if !greedy && (max == INFINITE || count < max) {
            self.push(
                Choice::LazyMore {
                    pc: self.pc,
                    pos,
                    count,
                },
                memory_limit,
            )?;
        }
        self.pos = pos;
        self.pc += 1;
        Ok(Step::Next)
    }

    /// The decision at the start of an iteration of a counted loop.
    fn loop_head(
        &mut self,
        reg: u32,
        min: u32,
        max: u32,
        greedy: bool,
        exit: u32,
        memory_limit: usize,
    ) -> Result<(), Error> {
        let count = self.read_slot(reg);
        if count < min {
            self.pc += 1;
        } else if max != INFINITE && count >= max {
            self.pc = exit;
        } else if greedy {
            self.push(
                Choice::Resume {
                    pc: exit,
                    pos: self.pos,
                },
                memory_limit,
            )?;
            self.pc += 1;
        } else {
            let pc = self.pc + 1;
            self.push(Choice::Resume { pc, pos: self.pos }, memory_limit)?;
            self.pc = exit;
        }
        Ok(())
    }

    /// The end of a lookaround body (§22.2.2.4): drops the choice points
    /// of the body (lookarounds are atomic) and keeps its captures if the
    /// lookaround is positive; a negative one fails.
    fn look_end(&mut self, reg: u32, negate: bool) -> Step {
        let depth = self.read_slot(reg) as usize;
        let Some(&Frame {
            choice: Choice::Look { pos, .. },
            ..
        }) = self.stack.get(depth)
        else {
            log::warn!("regular expression: a lookaround lost its choice point");
            return Step::Abort;
        };
        self.stack.truncate(depth);
        if negate {
            return Step::Fail;
        }
        self.pos = pos;
        self.pc += 1;
        Step::Next
    }

    fn assert<U: CodeUnit>(&self, input: &Input<'_, U>, a: Assertion) -> bool {
        let pos = self.pos as usize;
        let len = input.units.len();
        match a {
            Assertion::Start => pos == 0,
            Assertion::End => pos == len,
            Assertion::LineStart => {
                pos == 0
                    || input
                        .unit(pos - 1)
                        .is_some_and(|u| is_line_terminator(u32::from(u)))
            }
            Assertion::LineEnd => {
                pos == len
                    || input
                        .unit(pos)
                        .is_some_and(|u| is_line_terminator(u32::from(u)))
            }
            Assertion::WordBoundary { negate, extra } => {
                let a = input.is_word(pos.checked_sub(1), extra);
                let b = input.is_word(Some(pos), extra);
                (a != b) != negate
            }
        }
    }

    /// A backreference (`BackreferenceMatcher`, §22.2.2.7.2). `Some(true)`
    /// if it matched (the position moved), `None` if the steps ran out.
    fn backref<U: CodeUnit>(
        &mut self,
        program: &Program,
        input: &Input<'_, U>,
        list: u32,
        back: bool,
        ignore_case: bool,
        budget: &mut u32,
    ) -> Option<bool> {
        let groups = program
            .backref_lists
            .get(list as usize)
            .map_or(&[][..], |l| &l[..]);
        let span = groups.iter().find_map(|&g| {
            let s = Program::capture_slot(g);
            let (a, b) = (self.read_slot(s), self.read_slot(s + 1));
            (a != UNSET && b != UNSET).then_some((a as usize, b as usize))
        });
        let Some((start, end)) = span else {
            return Some(true);
        };
        let len = end - start;
        let pos = self.pos as usize;
        let from = if back {
            match pos.checked_sub(len) {
                Some(f) => f,
                None => return Some(false),
            }
        } else {
            if pos + len > input.units.len() {
                return Some(false);
            }
            pos
        };
        let mut done = match self.partial.take() {
            Some(Partial::Compare { done }) => done as usize,
            _ => 0,
        };
        let mut first = true;
        while done < len {
            if *budget == 0 && !first {
                self.partial = Some(Partial::Compare { done: done as u32 });
                return None;
            }
            *budget = budget.saturating_sub(1);
            first = false;
            let chunk_end = (done + 64).min(len);
            if !ignore_case {
                let a = input.units.get(start + done..start + chunk_end);
                let b = input.units.get(from + done..from + chunk_end);
                if a.is_none() || a != b {
                    return Some(false);
                }
                done = chunk_end;
                continue;
            }
            // With `i`, compare canonical characters; simple case folding
            // keeps a code point in its plane, so the widths match too.
            while done < chunk_end {
                let (Some((x, nx)), Some((y, ny))) =
                    (input.next(start + done), input.next(from + done))
                else {
                    return Some(false);
                };
                if nx - start != ny - from || !canonical_eq(program.case_mode, x, y) {
                    return Some(false);
                }
                done = nx - start;
            }
        }
        self.pos = if back { from } else { pos + len } as u32;
        Some(true)
    }
}

fn canonical_eq(mode: CaseMode, x: u32, y: u32) -> bool {
    x == y || charset::canonicalize(mode, x) == charset::canonicalize(mode, y)
}
