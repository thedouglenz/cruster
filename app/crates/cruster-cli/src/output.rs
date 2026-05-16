//! Output mode: which format to use and where to write.
//!
//! `--format` is honoured exactly when set. With no `--format`:
//! - `--llm` forces `ndjson`
//! - otherwise: TTY stdout → `text`, non-TTY → `ndjson`

use is_terminal::IsTerminal;

use crate::args::Format;

/// Decide the effective format from flags + TTY state.
pub fn effective_format(explicit: Option<Format>, llm: bool, is_tty: bool) -> Format {
    if let Some(f) = explicit {
        return f;
    }
    if llm || !is_tty {
        Format::Ndjson
    } else {
        Format::Text
    }
}

/// Convenience: is stdout currently a TTY?
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_format_wins() {
        assert!(matches!(
            effective_format(Some(Format::Yaml), false, true),
            Format::Yaml
        ));
        assert!(matches!(
            effective_format(Some(Format::Text), true, false),
            Format::Text
        ));
    }

    #[test]
    fn llm_implies_ndjson() {
        assert!(matches!(effective_format(None, true, true), Format::Ndjson));
    }

    #[test]
    fn non_tty_implies_ndjson() {
        assert!(matches!(
            effective_format(None, false, false),
            Format::Ndjson
        ));
    }

    #[test]
    fn tty_without_llm_is_text() {
        assert!(matches!(effective_format(None, false, true), Format::Text));
    }
}
