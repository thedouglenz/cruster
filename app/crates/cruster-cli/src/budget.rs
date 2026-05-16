//! Token-budget trimming for streaming output.
//!
//! Heuristic: 1 token ≈ 4 characters. Coarse but stable. Records are
//! emitted whole; the first record that would push the running total
//! past the budget is dropped and a `truncated` marker is emitted in
//! its place.

pub const CHARS_PER_TOKEN: usize = 4;

pub struct Budget {
    cap_chars: usize,
    used_chars: usize,
}

impl Budget {
    /// `tokens` is the user-facing budget; converted to a char cap.
    pub fn new(tokens: usize) -> Self {
        Self {
            cap_chars: tokens.saturating_mul(CHARS_PER_TOKEN),
            used_chars: 0,
        }
    }

    /// `true` if `additional_chars` fits within the remaining budget.
    pub fn fits(&self, additional_chars: usize) -> bool {
        self.used_chars + additional_chars <= self.cap_chars
    }

    /// Commit the chars to the running total.
    pub fn consume(&mut self, additional_chars: usize) {
        self.used_chars = self.used_chars.saturating_add(additional_chars);
    }

    pub fn remaining_chars(&self) -> usize {
        self.cap_chars.saturating_sub(self.used_chars)
    }

    pub fn used_chars(&self) -> usize {
        self.used_chars
    }

    pub fn cap_chars(&self) -> usize {
        self.cap_chars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_within_budget() {
        let b = Budget::new(100); // 400 chars
        assert!(b.fits(200));
    }

    #[test]
    fn rejects_overflow() {
        let mut b = Budget::new(100);
        b.consume(300);
        assert!(!b.fits(200));
        assert!(b.fits(100));
    }

    #[test]
    fn remaining_decreases_with_consume() {
        let mut b = Budget::new(100);
        assert_eq!(b.remaining_chars(), 400);
        b.consume(100);
        assert_eq!(b.remaining_chars(), 300);
    }
}
