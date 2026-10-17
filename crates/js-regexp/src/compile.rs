//! The compiler from the pattern tree to the backtracking program
//! (§22.2.2, `CompilePattern` and `CompileSubpattern`; ADR 0026 section 10).
//!
//! It walks the tree with an explicit stack of tasks, never with Rust
//! recursion. A lookbehind compiles its body backward: the terms of each
//! sequence in reverse order, with instructions that read backward
//! (§22.2.2.3.4). A quantifier over one character without captures is a
//! single loop instruction; other quantifiers are loops with a counter
//! register, a capture reset per iteration when the atom has groups and an
//! empty check when the atom can match the empty string (§22.2.2.3.1).

use crate::ast::{Ast, INFINITE, Node, NodeId};
use crate::charset::CharSet;
use crate::error::{Error, SyntaxKind};
use crate::program::{Inst, Prefix, Program, SetData, Test};
use crate::{Flags, charset};

/// The most instructions a program may have.
pub(crate) const MAX_INSTRUCTIONS: usize = 1 << 22;

/// A step of the compiler.
enum Task {
    /// Compile a node.
    Node { id: NodeId, back: bool },
    /// Emit an instruction.
    Emit(Inst),
    /// Between two alternatives: the jump to the end of the alternation,
    /// and the fork to the next alternative unless it is the last.
    AltBetween { state: usize, last: bool },
    /// After the last alternative: patch the jumps.
    AltEnd { state: usize },
    /// After a lookaround body: emit its end and patch its start.
    LookEnd { start: u32, reg: u32, negate: bool },
    /// After a loop body.
    LoopEnd { head: u32, tail: Tail },
}

/// The end of a loop body.
enum Tail {
    /// A loop without a counter (`*`, `?`): an optional empty check, then
    /// for `*` a jump back.
    Plain { progress: Option<u32>, repeat: bool },
    /// A counted loop.
    Counted {
        reg: u32,
        min: u32,
        empty_check: bool,
    },
}

struct Compiler<'a> {
    ast: &'a Ast,
    insts: Vec<Inst>,
    /// The next free slot.
    next_slot: u32,
    alternations: Vec<(u32, Vec<u32>)>,
}

/// Compiles a parsed pattern.
pub(crate) fn compile(ast: &Ast, flags: Flags) -> Result<Program, Error> {
    if ast.uses_properties {
        return Err(Error::Unsupported("Unicode property escapes"));
    }
    let unicode = flags.contains(Flags::UNICODE) || flags.contains(Flags::UNICODE_SETS);
    let mut compiler = Compiler {
        ast,
        insts: Vec::new(),
        next_slot: (ast.group_count + 1) * 2 + ast.group_count,
        alternations: Vec::new(),
    };
    compiler.run(ast.root)?;
    compiler.emit(Inst::Match)?;
    let sticky = flags.contains(Flags::STICKY);
    Ok(Program {
        insts: compiler.insts,
        sets: ast.sets.iter().map(|s| SetData::new(s.ranges())).collect(),
        backref_lists: ast
            .backref_lists
            .iter()
            .map(|l| l.clone().into_boxed_slice())
            .collect(),
        group_count: ast.group_count,
        slot_count: compiler.next_slot,
        unicode,
        case_mode: if unicode {
            charset::CaseMode::Fold
        } else {
            charset::CaseMode::Upper
        },
        sticky,
        prefix: if sticky { None } else { prefix(ast) },
    })
}

impl Compiler<'_> {
    fn emit(&mut self, inst: Inst) -> Result<u32, Error> {
        if self.insts.len() >= MAX_INSTRUCTIONS {
            return Err(Error::Syntax(SyntaxKind::TooLarge));
        }
        self.insts.push(inst);
        Ok((self.insts.len() - 1) as u32)
    }

    fn pc(&self) -> u32 {
        self.insts.len() as u32
    }

    fn slot(&mut self, n: u32) -> u32 {
        let slot = self.next_slot;
        self.next_slot += n;
        slot
    }

    /// Sets the target of the jump or fork at `at`.
    fn patch(&mut self, at: u32, target: u32) {
        if let Some(inst) = self.insts.get_mut(at as usize) {
            match inst {
                Inst::Fork { alt } => *alt = target,
                Inst::ForkFirst { first } => *first = target,
                Inst::Jump { to } => *to = target,
                Inst::LookStart { end, .. } => *end = target,
                Inst::LoopHead { exit, .. } => *exit = target,
                _ => {}
            }
        }
    }

    fn run(&mut self, root: NodeId) -> Result<(), Error> {
        let mut tasks = vec![Task::Node {
            id: root,
            back: false,
        }];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Node { id, back } => self.node(id, back, &mut tasks)?,
                Task::Emit(inst) => {
                    self.emit(inst)?;
                }
                Task::AltBetween { state, last } => {
                    let jump = self.emit(Inst::Jump { to: 0 })?;
                    let pc = self.pc();
                    let fork = self.alternations.get(state).map_or(0, |s| s.0);
                    self.patch(fork, pc);
                    let next_fork = if last {
                        0
                    } else {
                        self.emit(Inst::Fork { alt: 0 })?
                    };
                    if let Some(s) = self.alternations.get_mut(state) {
                        s.0 = next_fork;
                        s.1.push(jump);
                    }
                }
                Task::AltEnd { state } => {
                    let pc = self.pc();
                    let jumps = self
                        .alternations
                        .get_mut(state)
                        .map(|s| std::mem::take(&mut s.1))
                        .unwrap_or_default();
                    for jump in jumps {
                        self.patch(jump, pc);
                    }
                }
                Task::LookEnd { start, reg, negate } => {
                    self.emit(Inst::LookEnd { reg, negate })?;
                    let pc = self.pc();
                    self.patch(start, pc);
                }
                Task::LoopEnd { head, tail } => {
                    match tail {
                        Tail::Plain { progress, repeat } => {
                            if let Some(reg) = progress {
                                self.emit(Inst::CheckProgress { reg })?;
                            }
                            if repeat {
                                self.emit(Inst::Jump { to: head })?;
                            }
                        }
                        Tail::Counted {
                            reg,
                            min,
                            empty_check,
                        } => {
                            self.emit(Inst::LoopTail {
                                reg,
                                min,
                                head,
                                empty_check,
                            })?;
                        }
                    }
                    let pc = self.pc();
                    self.patch(head, pc);
                }
            }
        }
        Ok(())
    }

    fn test_of(&self, id: NodeId) -> Option<Test> {
        match *self.ast.node(id) {
            Node::Char(c) => Some(Test::Char(c)),
            Node::Set(i) => Some(Test::Set(i)),
            Node::Any => Some(Test::Any),
            Node::AnyButLineTerminator => Some(Test::AnyButLineTerminator),
            _ => None,
        }
    }

    fn node(&mut self, id: NodeId, back: bool, tasks: &mut Vec<Task>) -> Result<(), Error> {
        if let Some(test) = self.test_of(id) {
            self.emit(Inst::Test { test, back })?;
            return Ok(());
        }
        match self.ast.node(id) {
            Node::Empty | Node::Char(_) | Node::Set(_) | Node::Any | Node::AnyButLineTerminator => {
            }
            Node::Assert(a) => {
                self.emit(Inst::Assert(*a))?;
            }
            Node::Backref { list, ignore_case } => {
                self.emit(Inst::Backref {
                    list: *list,
                    back,
                    ignore_case: *ignore_case,
                })?;
            }
            Node::Group { capture, child } => {
                if let Some(group) = *capture {
                    self.emit(Inst::Open { group })?;
                    tasks.push(Task::Emit(Inst::Close { group }));
                }
                tasks.push(Task::Node { id: *child, back });
            }
            Node::Look {
                behind,
                negate,
                child,
            } => {
                let reg = self.slot(1);
                let start = self.emit(Inst::LookStart {
                    reg,
                    negate: *negate,
                    end: 0,
                })?;
                tasks.push(Task::LookEnd {
                    start,
                    reg,
                    negate: *negate,
                });
                tasks.push(Task::Node {
                    id: *child,
                    back: *behind,
                });
            }
            Node::Seq(items) => {
                if back {
                    tasks.extend(items.iter().map(|&id| Task::Node { id, back }));
                } else {
                    tasks.extend(items.iter().rev().map(|&id| Task::Node { id, back }));
                }
            }
            Node::Alt(items) => {
                let Some((&last, rest)) = items.split_last() else {
                    return Ok(());
                };
                let fork = self.emit(Inst::Fork { alt: 0 })?;
                let state = self.alternations.len();
                self.alternations.push((fork, Vec::new()));
                tasks.push(Task::AltEnd { state });
                tasks.push(Task::Node { id: last, back });
                for (i, &item) in rest.iter().enumerate().rev() {
                    tasks.push(Task::AltBetween {
                        state,
                        last: i + 1 == rest.len(),
                    });
                    tasks.push(Task::Node { id: item, back });
                }
            }
            Node::Repeat {
                min,
                max,
                greedy,
                child,
                first_group,
                group_count,
            } => self.repeat(
                &Repeat {
                    min: *min,
                    max: *max,
                    greedy: *greedy,
                    child: *child,
                    first_group: *first_group,
                    group_count: *group_count,
                },
                back,
                tasks,
            )?,
        }
        Ok(())
    }

    fn repeat(&mut self, r: &Repeat, back: bool, tasks: &mut Vec<Task>) -> Result<(), Error> {
        let info = self.ast.info(r.child);
        let (mut min, mut max) = (r.min, r.max);
        if info.zero_width {
            // Every iteration matches the empty string: the first one is
            // enough when there is a minimum, and with none an iteration
            // always fails the empty check.
            min = min.min(1);
            max = min;
        }
        if max == 0 {
            return Ok(());
        }
        let reset = Inst::Reset {
            first: r.first_group,
            count: r.group_count,
        };
        if min == 1 && max == 1 {
            if r.group_count > 0 {
                self.emit(reset)?;
            }
            tasks.push(Task::Node { id: r.child, back });
            return Ok(());
        }
        if r.group_count == 0
            && let Some(test) = self.test_of(r.child)
        {
            self.emit(Inst::Repeat {
                test,
                back,
                greedy: r.greedy,
                min,
                max,
            })?;
            return Ok(());
        }
        let can_be_empty = info.min_len == 0;
        if min == 0 && (max == INFINITE || max == 1) {
            let head = if r.greedy {
                self.emit(Inst::Fork { alt: 0 })?
            } else {
                self.emit(Inst::ForkFirst { first: 0 })?
            };
            let progress = if can_be_empty {
                let reg = self.slot(1);
                self.emit(Inst::SavePos { reg })?;
                Some(reg)
            } else {
                None
            };
            if r.group_count > 0 {
                self.emit(reset)?;
            }
            tasks.push(Task::LoopEnd {
                head,
                tail: Tail::Plain {
                    progress,
                    repeat: max == INFINITE,
                },
            });
            tasks.push(Task::Node { id: r.child, back });
            return Ok(());
        }
        let reg = self.slot(2);
        self.emit(Inst::LoopInit { reg })?;
        let head = self.emit(Inst::LoopHead {
            reg,
            min,
            max,
            greedy: r.greedy,
            exit: 0,
        })?;
        if can_be_empty {
            self.emit(Inst::SavePos { reg: reg + 1 })?;
        }
        if r.group_count > 0 {
            self.emit(reset)?;
        }
        tasks.push(Task::LoopEnd {
            head,
            tail: Tail::Counted {
                reg,
                min,
                empty_check: can_be_empty,
            },
        });
        tasks.push(Task::Node { id: r.child, back });
        Ok(())
    }
}

/// The fields of [`Node::Repeat`].
struct Repeat {
    min: u32,
    max: u32,
    greedy: bool,
    child: NodeId,
    first_group: u32,
    group_count: u32,
}

/// The set of characters that can start a match, if every match starts
/// with a character (`None` if a match can be empty). Walks at most a few
/// hundred nodes.
fn prefix(ast: &Ast) -> Option<Prefix> {
    const BUDGET: usize = 256;
    let mut set = CharSet::new();
    let mut work = vec![ast.root];
    let mut visited = 0;
    while let Some(id) = work.pop() {
        visited += 1;
        if visited > BUDGET {
            return None;
        }
        match ast.node(id) {
            Node::Char(c) => set.add(&CharSet::single(*c)),
            Node::Set(i) => set.add(ast.sets.get(*i as usize)?),
            Node::Group { child, .. } => work.push(*child),
            Node::Repeat { min, child, .. } if *min > 0 => work.push(*child),
            Node::Seq(items) => {
                // Zero-width terms (assertions, lookarounds) consume
                // nothing: the first character comes from the next term.
                let first = *items.iter().find(|&&id| !ast.info(id).zero_width)?;
                if ast.info(first).min_len == 0 {
                    return None;
                }
                work.push(first);
            }
            Node::Alt(items) => work.extend(items.iter().copied()),
            _ => return None,
        }
    }
    if let Some(c) = set.single_char() {
        return Some(Prefix::Char(c));
    }
    if set.is_empty() {
        return None;
    }
    Some(Prefix::Set(SetData::new(set.ranges())))
}
