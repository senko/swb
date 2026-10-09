//! The shared recursion budget of the JavaScript engine (ADR 0026
//! section 9).
//!
//! All Rust recursion in the engine charges one budget: the parser per
//! nesting level and per AST level, later the compiler, native re-entry
//! into the interpreter, JSON, Proxy traps and the regular expression
//! parser. Each kind of recursion charges a weight that approximates its
//! stack use in bytes in a debug build, so the budget is a number of
//! stack bytes. The budget assumes a thread stack of at least 8 MiB.
//!
//! This type lives in `js-text` because all engine crates depend on this
//! crate and the regular expression parser (`js-regexp`) cannot depend on
//! `js-syntax`.

/// A recursion budget in approximate bytes of stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecursionBudget {
    remaining: u32,
}

/// The budget was exhausted. JavaScript code sees it as a `RangeError`
/// ("Maximum call stack size exceeded"), or as the error that Chromium
/// gives for the same input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetExhausted;

impl RecursionBudget {
    /// The budget of a thread with an 8 MiB stack: half of the stack, so
    /// that frames which do not charge the budget (leaf functions, the
    /// lexer, allocation, formatting) and the host's own frames below the
    /// engine have room.
    pub const DEFAULT: RecursionBudget = RecursionBudget::new(4 << 20);

    /// A budget of `bytes` approximate stack bytes.
    pub const fn new(bytes: u32) -> Self {
        RecursionBudget { remaining: bytes }
    }

    /// The bytes that remain.
    pub const fn remaining(self) -> u32 {
        self.remaining
    }

    /// Charges `weight` bytes for one level of recursion. Fails without a
    /// change if the budget does not have them.
    pub fn enter(&mut self, weight: u32) -> Result<(), BudgetExhausted> {
        match self.remaining.checked_sub(weight) {
            Some(rest) => {
                self.remaining = rest;
                Ok(())
            }
            None => Err(BudgetExhausted),
        }
    }

    /// Gives back `weight` bytes when a level of recursion returns.
    pub fn leave(&mut self, weight: u32) {
        self.remaining = self.remaining.saturating_add(weight);
    }
}

impl Default for RecursionBudget {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_and_leave_balance() {
        let mut budget = RecursionBudget::new(100);
        assert_eq!(budget.enter(60), Ok(()));
        assert_eq!(budget.enter(60), Err(BudgetExhausted));
        assert_eq!(budget.remaining(), 40);
        assert_eq!(budget.enter(40), Ok(()));
        assert_eq!(budget.remaining(), 0);
        budget.leave(40);
        budget.leave(60);
        assert_eq!(budget.remaining(), 100);
        assert_eq!(RecursionBudget::default(), RecursionBudget::DEFAULT);
    }
}
