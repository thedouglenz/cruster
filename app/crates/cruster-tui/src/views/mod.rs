//! Resource views.

pub mod configmaps;
pub mod daemonsets;
pub mod dashboard;
pub mod deployments;
pub mod events;
pub mod namespaces;
pub mod nodes;
pub mod pods;
pub mod secrets;
pub mod services;
pub mod statefulsets;

use ratatui::style::{Modifier, Style};

use crate::theme::Theme;

/// Muted-red tint applied to every cell of a row that represents an
/// unhealthy resource (0/N Ready, NotReady node, Failed pod phase).
/// Per-cell highlights (e.g. the bold-red READY cell) stack on top
/// via `merge_styles`. Selection highlight still wins via Table's
/// row-style override.
pub(crate) fn unhealthy_row_style(theme: &Theme) -> Style {
    Style::default().fg(theme.status.failed.as_ratatui())
}

/// Combine an optional row tint with a per-cell style. The cell
/// style's fg/modifier win when set; the row tint fills in fg when
/// the cell didn't specify one. ratatui's row-level `.style()` does
/// NOT propagate to cells whose own style differs, so we compose
/// explicitly at the cell level.
pub(crate) fn merge_styles(row_tint: Option<Style>, cell: Style) -> Style {
    let Some(t) = row_tint else {
        return cell;
    };
    Style {
        fg: cell.fg.or(t.fg),
        bg: cell.bg.or(t.bg),
        add_modifier: cell.add_modifier | t.add_modifier,
        sub_modifier: cell.sub_modifier | t.sub_modifier,
        underline_color: cell.underline_color.or(t.underline_color),
    }
}

/// Heuristic: does an "X/Y" READY string mean the row is unhealthy?
/// `0/N` for N > 0 is the canonical "broken" case. `0/0` (no
/// containers) and any partial/full ready value don't count.
pub(crate) fn ready_str_is_unhealthy(ready: &str) -> bool {
    let Some((num, den)) = ready.split_once('/') else {
        return false;
    };
    let Ok(n): Result<u32, _> = num.trim().parse() else {
        return false;
    };
    let Ok(d): Result<u32, _> = den.trim().parse() else {
        return false;
    };
    n == 0 && d > 0
}

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

    #[test]
    fn ready_str_is_unhealthy_recognises_zero_over_n() {
        assert!(ready_str_is_unhealthy("0/1"));
        assert!(ready_str_is_unhealthy("0/3"));
        assert!(!ready_str_is_unhealthy("1/3"));
        assert!(!ready_str_is_unhealthy("3/3"));
        assert!(!ready_str_is_unhealthy("0/0"));
        assert!(!ready_str_is_unhealthy("?"));
        assert!(!ready_str_is_unhealthy(""));
    }

    #[test]
    fn unhealthy_row_style_uses_status_failed_fg() {
        let s = unhealthy_row_style(&theme());
        assert_eq!(s.fg, Some(ratatui::style::Color::Red));
        // Not bold — the cell-level emphasis (ready_cell_style) is
        // bold; the row tint is meant to be muted enough to layer
        // under the cell highlight without competing.
        assert!(!s.add_modifier.contains(Modifier::BOLD));
    }
}
