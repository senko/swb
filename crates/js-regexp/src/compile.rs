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
    tables: Vec<Box<[(u32, u32)]>>,
}

/// Compiles a parsed pattern.
pub(crate) fn compile(ast: &Ast, flags: Flags) -> Result<Program, Error> {
    let unicode = flags.contains(Flags::UNICODE) || flags.contains(Flags::UNICODE_SETS);
    let mut compiler = Compiler {
        ast,
        insts: Vec::new(),
        next_slot: (ast.group_count + 1) * 2 + ast.group_count,
        alternations: Vec::new(),
        tables: Vec::new(),
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
        tables: compiler.tables,
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
            Node::Strings { list, fold } => self.strings(*list, *fold, back)?,
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

    /// Compiles a list of strings as a trie (see [`Trie`]): an
    /// [`Inst::Switch`] per node with children, preceded by a fork to the
    /// end if a string ends at the node.
    ///
    /// The strings that match at one position are prefixes of each other
    /// (suffixes, when reading backward), so they lie on one path of the
    /// trie. The alternatives of a class with strings are tried from the
    /// longest string to the shortest (`CompileAtom`, §22.2.2.7, and the
    /// order of `CompileToCharSet`, §22.2.2.9). The trie goes down as far
    /// as the input allows first, and the fork at a node where a string
    /// ends is a choice point that is taken only after everything deeper
    /// failed: the same order, also when a later part of the pattern
    /// fails and the matcher backtracks into the strings. A lookbehind
    /// reads the strings from their ends, so its trie holds the reversed
    /// strings.
    fn strings(&mut self, list: u32, fold: bool, back: bool) -> Result<(), Error> {
        let ast = self.ast;
        let Some(strings) = ast.string_lists.get(list as usize) else {
            return Ok(());
        };
        let trie = Trie::new(strings, fold, back);
        let mut pcs = vec![0u32; trie.nodes.len()];
        let mut table_ids = vec![0u32; trie.nodes.len()];
        let mut forks = Vec::new();
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let Some(node) = trie.nodes.get(n) else {
                continue;
            };
            if node.edges.is_empty() {
                continue;
            }
            if let Some(pc) = pcs.get_mut(n) {
                *pc = self.pc();
            }
            if node.accept {
                forks.push(self.emit(Inst::Fork { alt: 0 })?);
            }
            let table = self.tables.len() as u32;
            self.tables.push(Box::new([]));
            if let Some(id) = table_ids.get_mut(n) {
                *id = table;
            }
            self.emit(Inst::Switch { table, back, fold })?;
            stack.extend(node.edges.iter().rev().map(|e| e.1 as usize));
        }
        let end = self.pc();
        for fork in forks {
            self.patch(fork, end);
        }
        for (n, node) in trie.nodes.iter().enumerate() {
            if node.edges.is_empty() {
                continue;
            }
            let entries: Box<[(u32, u32)]> = node
                .edges
                .iter()
                .map(|&(c, child)| {
                    let leaf = trie
                        .nodes
                        .get(child as usize)
                        .is_none_or(|t| t.edges.is_empty());
                    let target = if leaf {
                        end
                    } else {
                        pcs.get(child as usize).copied().unwrap_or(end)
                    };
                    (c, target)
                })
                .collect();
            let id = table_ids.get(n).copied().unwrap_or(0) as usize;
            if let Some(slot) = self.tables.get_mut(id) {
                *slot = entries;
            }
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

/// A node of the trie of a list of strings.
struct TrieNode {
    /// A string ends here.
    accept: bool,
    /// (character, index of the child node), sorted by character.
    edges: Vec<(u32, u32)>,
}

/// The trie of a list of strings. The node 0 is the root.
struct Trie {
    nodes: Vec<TrieNode>,
}

impl Trie {
    /// With `fold`, the strings are case folded (the matcher folds the
    /// input); with `back`, they are reversed.
    fn new(strings: &[Vec<u32>], fold: bool, back: bool) -> Trie {
        let mut keys: Vec<Vec<u32>> = strings
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut key: Vec<u32> = s
                    .iter()
                    .map(|&c| {
                        if fold {
                            charset::canonicalize(charset::CaseMode::Fold, c)
                        } else {
                            c
                        }
                    })
                    .collect();
                if back {
                    key.reverse();
                }
                key
            })
            .collect();
        // In sorted order, a new edge of a node never comes before the
        // edges that it has.
        keys.sort_unstable();
        keys.dedup();
        let mut nodes = vec![TrieNode {
            accept: false,
            edges: Vec::new(),
        }];
        for key in &keys {
            let mut at = 0usize;
            for &c in key {
                let last = nodes.get(at).and_then(|n| n.edges.last().copied());
                at = match last {
                    Some((x, child)) if x == c => child as usize,
                    _ => {
                        let child = nodes.len();
                        nodes.push(TrieNode {
                            accept: false,
                            edges: Vec::new(),
                        });
                        if let Some(node) = nodes.get_mut(at) {
                            node.edges.push((c, child as u32));
                        }
                        child
                    }
                };
            }
            if let Some(node) = nodes.get_mut(at) {
                node.accept = true;
            }
        }
        Trie { nodes }
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
            Node::Strings { list, fold } => {
                let strings = ast.string_lists.get(*list as usize)?;
                let mut firsts =
                    CharSet::from_ranges(strings.iter().filter_map(|s| s.first()).map(|&c| (c, c)));
                if *fold {
                    firsts = charset::case_closure(charset::CaseMode::Fold, &firsts);
                }
                set.add(&firsts);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trie_shares_prefixes_and_marks_ends() {
        let strings = vec![
            vec![1, 2, 3],
            vec![1, 2],
            vec![1, 5],
            vec![4, 2],
            vec![1, 2],
        ];
        let trie = Trie::new(&strings, false, false);
        // root, 1, 1-2, 1-2-3, 1-5, 4, 4-2
        assert_eq!(trie.nodes.len(), 7);
        let root = &trie.nodes[0];
        assert_eq!(root.edges.iter().map(|e| e.0).collect::<Vec<_>>(), [1, 4]);
        let one_two = &trie.nodes[trie.nodes[1].edges[0].1 as usize];
        assert!(one_two.accept && !one_two.edges.is_empty());
        assert!(!trie.nodes[1].accept);
    }

    #[test]
    fn trie_reverses_and_folds() {
        let trie = Trie::new(&[vec![0x41, 0x42], vec![0x61, 0x62]], true, true);
        // Folded to "ab" (one string), reversed: b, a.
        assert_eq!(trie.nodes.len(), 3);
        assert_eq!(trie.nodes[0].edges[0].0, 0x62);
    }
}
