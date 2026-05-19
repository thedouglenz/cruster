//! Resource views.

pub mod configmaps;
pub mod dashboard;
pub mod deployments;
pub mod events;
pub mod namespaces;
pub mod nodes;
pub mod pods;
pub mod secrets;
pub mod services;

use ratatui::style::{Modifier, Style};

use crate::theme::Theme;

/// Style for a READY-column cell whose content is a "X/Y" string.
///
/// When X parses as 0 and Y > 0, returns a bold `status.failed`
/// style. Otherwise returns `Style::default()`. Centralised so every
/// workload list view paints the cue identically.
pub(crate) fn ready_cell_style(ready: &str, theme: &Theme) -> Style {
    let (num, den) = match ready.split_once('/') {
        Some((n, d)) => (n.trim(), d.trim()),
        None => return Style::default(),
    };
    let Ok(n): Result<u32, _> = num.parse() else {
        return Style::default();
    };
    let Ok(d): Result<u32, _> = den.parse() else {
        return Style::default();
    };
    if n == 0 && d > 0 {
        Style::default()
            .fg(theme.status.failed.as_ratatui())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

#[cfg(test)]
mod ready_cell_tests {
    use super::*;
    use ratatui::style::Color;

    fn theme() -> Theme {
        Theme::terminal_default()
    }

    #[test]
    fn zero_over_n_is_failed_bold() {
        let s = ready_cell_style("0/3", &theme());
        assert_eq!(s.fg, Some(Color::Red));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn partial_ready_is_default() {
        let s = ready_cell_style("1/3", &theme());
        assert_eq!(s.fg, None);
        assert!(!s.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn fully_ready_is_default() {
        let s = ready_cell_style("3/3", &theme());
        assert_eq!(s.fg, None);
    }

    #[test]
    fn zero_over_zero_is_default() {
        // A pod with no containers reads "0/0" — that's not
        // "broken", it's "nothing to be ready about".
        let s = ready_cell_style("0/0", &theme());
        assert_eq!(s.fg, None);
    }

    #[test]
    fn malformed_input_is_default() {
        assert_eq!(ready_cell_style("?", &theme()).fg, None);
        assert_eq!(ready_cell_style("", &theme()).fg, None);
        assert_eq!(ready_cell_style("0", &theme()).fg, None);
        assert_eq!(ready_cell_style("a/b", &theme()).fg, None);
    }
}
