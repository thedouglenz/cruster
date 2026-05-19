//! Help overlay: a `?`-triggered cheat sheet showing every keybind
//! the current keymap preset exposes, grouped by category.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::keymap::{Keymap, SemanticAction};
use crate::overlay::{Overlay, OverlayResult};
use crate::overlays::chrome::centered_rect;
use crate::theme::Theme;

/// Display grouping for the help cheat sheet. Order matters: rendered
/// top-to-bottom in this enum's declared order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Navigation,
    Views,
    Actions,
    Modes,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Navigation => "Navigation",
            Category::Views => "Views",
            Category::Actions => "Actions on selection",
            Category::Modes => "Modes",
        }
    }

    pub fn order() -> [Category; 4] {
        [
            Category::Navigation,
            Category::Views,
            Category::Actions,
            Category::Modes,
        ]
    }
}

/// Single source of truth for which category each semantic action
/// belongs to. Every variant of `SemanticAction` MUST appear here —
/// `match` exhaustiveness keeps it honest at compile time.
pub fn category_for(action: SemanticAction) -> Category {
    match action {
        SemanticAction::Quit
        | SemanticAction::MoveUp
        | SemanticAction::MoveDown
        | SemanticAction::MoveTop
        | SemanticAction::MoveBottom => Category::Navigation,

        SemanticAction::OpenPalette
        | SemanticAction::OpenSearch
        | SemanticAction::OpenCommandMode
        | SemanticAction::OpenHistory
        | SemanticAction::OpenWorkflows
        | SemanticAction::OpenThemes
        | SemanticAction::OpenRelationships
        | SemanticAction::OpenHelp => Category::Views,

        SemanticAction::Describe
        | SemanticAction::Logs
        | SemanticAction::Exec
        | SemanticAction::PortForward
        | SemanticAction::EditYaml
        | SemanticAction::Delete
        | SemanticAction::CopyKubectl
        | SemanticAction::PinToDashboard => Category::Actions,

        SemanticAction::ToggleReadOnly
        | SemanticAction::LayoutSingle
        | SemanticAction::LayoutTriplet
        | SemanticAction::LayoutIncident
        | SemanticAction::OpenPromptLeader
        | SemanticAction::ExportDiagnostic => Category::Modes,
    }
}

/// Human-readable label for the action — what shows up next to the
/// chord in the cheat sheet.
pub fn label_for(action: SemanticAction) -> &'static str {
    match action {
        SemanticAction::Quit => "Quit",
        SemanticAction::MoveUp => "Move up",
        SemanticAction::MoveDown => "Move down",
        SemanticAction::MoveTop => "Top",
        SemanticAction::MoveBottom => "Bottom",
        SemanticAction::OpenPalette => "Command palette",
        SemanticAction::OpenSearch => "Search / filter",
        SemanticAction::OpenCommandMode => "Switch view (`:alias`)",
        SemanticAction::OpenHistory => "Recent views",
        SemanticAction::OpenWorkflows => "Workflows",
        SemanticAction::OpenThemes => "Themes",
        SemanticAction::OpenRelationships => "Relationships",
        SemanticAction::OpenHelp => "This help",
        SemanticAction::Describe => "Describe / YAML",
        SemanticAction::Logs => "Logs",
        SemanticAction::Exec => "Exec into pod",
        SemanticAction::PortForward => "Port-forward",
        SemanticAction::EditYaml => "Edit YAML",
        SemanticAction::Delete => "Delete (modal)",
        SemanticAction::CopyKubectl => "Copy as kubectl",
        SemanticAction::PinToDashboard => "Pin to dashboard",
        SemanticAction::ToggleReadOnly => "Toggle read-only",
        SemanticAction::LayoutSingle => "Layout: single",
        SemanticAction::LayoutTriplet => "Layout: triplet",
        SemanticAction::LayoutIncident => "Layout: incident",
        SemanticAction::OpenPromptLeader => "Prompt leader (Pro)",
        SemanticAction::ExportDiagnostic => "Export diagnostic (Pro)",
    }
}

/// Render a keychord like `Ctrl+P` / `Alt+1` / `?` from a code+mods
/// pair. Mirrors the action-footer formatter but spells modifiers out.
pub fn format_chord(code: KeyCode, mods: KeyModifiers) -> String {
    let mut s = String::new();
    if mods.contains(KeyModifiers::CONTROL) {
        s.push_str("Ctrl+");
    }
    if mods.contains(KeyModifiers::ALT) {
        s.push_str("Alt+");
    }
    if mods.contains(KeyModifiers::SHIFT) && !matches!(code, KeyCode::Char(c) if c.is_uppercase()) {
        s.push_str("Shift+");
    }
    match code {
        KeyCode::Char(c) => s.push(c),
        KeyCode::Esc => s.push_str("Esc"),
        KeyCode::Enter => s.push_str("Enter"),
        KeyCode::Tab => s.push_str("Tab"),
        KeyCode::Up => s.push('↑'),
        KeyCode::Down => s.push('↓'),
        KeyCode::Left => s.push('←'),
        KeyCode::Right => s.push('→'),
        KeyCode::Home => s.push_str("Home"),
        KeyCode::End => s.push_str("End"),
        other => s.push_str(&format!("{other:?}")),
    }
    s
}

pub struct HelpOverlay {
    keymap: Keymap,
}

impl HelpOverlay {
    pub fn new(keymap: Keymap) -> Self {
        Self { keymap }
    }

    /// Group the keymap into `(category, [(chord, label), ...])`
    /// pairs, sorted within each category by label.
    pub fn grouped(&self) -> Vec<(Category, Vec<(String, &'static str)>)> {
        let mut buckets: Vec<(Category, Vec<(String, &'static str)>)> =
            Category::order().iter().map(|c| (*c, Vec::new())).collect();

        for ((code, mods), action) in self.keymap.entries() {
            let cat = category_for(*action);
            let bucket = buckets
                .iter_mut()
                .find(|(c, _)| *c == cat)
                .expect("category bucket exists");
            bucket
                .1
                .push((format_chord(*code, *mods), label_for(*action)));
        }

        // Within a category, sort by label so the layout is stable
        // regardless of HashMap iteration order.
        for (_, rows) in &mut buckets {
            rows.sort_by(|a, b| a.1.cmp(b.1));
            rows.dedup_by(|a, b| a.1 == b.1);
        }
        buckets
    }
}

impl Overlay for HelpOverlay {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => OverlayResult::Close,
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let theme = Theme::terminal_default();
        self.render_with_theme(frame, area, &theme);
    }
}

impl HelpOverlay {
    pub fn render_with_theme(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let groups = self.grouped();
        let total_rows: u16 = groups
            .iter()
            .map(|(_, rows)| rows.len() as u16 + 2)
            .sum::<u16>()
            + 2;
        let Some(rect) = centered_rect(area, 40, 10, 64, total_rows.min(30)) else {
            return;
        };
        frame.render_widget(Clear, rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.overlay_border.as_ratatui()))
            .title(" Cruster keys ")
            .title_style(Style::default().add_modifier(Modifier::BOLD));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let muted = Style::default().fg(theme.muted_fg.as_ratatui());
        let header_style = Style::default()
            .fg(theme.header_fg.as_ratatui())
            .add_modifier(Modifier::BOLD);
        let chord_style = Style::default()
            .fg(theme.selection_fg.as_ratatui())
            .add_modifier(Modifier::BOLD);

        let constraints: Vec<Constraint> = groups
            .iter()
            .flat_map(|(_, rows)| {
                vec![
                    Constraint::Length(1),
                    Constraint::Length(rows.len() as u16),
                    Constraint::Length(1),
                ]
            })
            .chain(std::iter::once(Constraint::Min(0)))
            .collect();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        let mut slot = 0;
        for (cat, rows) in &groups {
            // Category header.
            frame.render_widget(
                Paragraph::new(format!("  {}", cat.label())).style(header_style),
                chunks[slot],
            );
            slot += 1;
            // Rows.
            let lines: Vec<Line> = rows
                .iter()
                .map(|(chord, label)| {
                    Line::from(vec![
                        Span::raw("    "),
                        Span::styled(format!("{chord:<10}  "), chord_style),
                        Span::styled(label.to_string(), muted),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), chunks[slot]);
            slot += 1;
            // Spacer.
            slot += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Keymap;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Compile-time + runtime safety: every variant of SemanticAction
    /// has a category, label, and a chord we can format. If a new
    /// variant is added without updating this list, the match in
    /// category_for refuses to compile — and this test catches
    /// missing labels.
    #[test]
    fn every_semantic_action_has_a_category_and_label() {
        let all = [
            SemanticAction::Quit,
            SemanticAction::MoveUp,
            SemanticAction::MoveDown,
            SemanticAction::MoveTop,
            SemanticAction::MoveBottom,
            SemanticAction::OpenPalette,
            SemanticAction::OpenSearch,
            SemanticAction::OpenCommandMode,
            SemanticAction::OpenHistory,
            SemanticAction::OpenWorkflows,
            SemanticAction::OpenThemes,
            SemanticAction::OpenRelationships,
            SemanticAction::OpenHelp,
            SemanticAction::Describe,
            SemanticAction::Logs,
            SemanticAction::Exec,
            SemanticAction::PortForward,
            SemanticAction::EditYaml,
            SemanticAction::Delete,
            SemanticAction::CopyKubectl,
            SemanticAction::PinToDashboard,
            SemanticAction::ToggleReadOnly,
            SemanticAction::LayoutSingle,
            SemanticAction::LayoutTriplet,
            SemanticAction::LayoutIncident,
            SemanticAction::OpenPromptLeader,
            SemanticAction::ExportDiagnostic,
        ];
        for a in all {
            let _ = category_for(a);
            let label = label_for(a);
            assert!(!label.is_empty(), "empty label for {:?}", a);
        }
    }

    #[test]
    fn grouped_keymap_includes_delete_under_actions() {
        let o = HelpOverlay::new(Keymap::normal());
        let groups = o.grouped();
        let actions = groups
            .iter()
            .find(|(c, _)| *c == Category::Actions)
            .unwrap();
        assert!(
            actions.1.iter().any(|(_, l)| *l == "Delete (modal)"),
            "Actions group should contain Delete (modal): {:?}",
            actions.1
        );
    }

    #[test]
    fn grouped_keymap_includes_help_under_views() {
        let o = HelpOverlay::new(Keymap::normal());
        let groups = o.grouped();
        let views = groups.iter().find(|(c, _)| *c == Category::Views).unwrap();
        assert!(
            views.1.iter().any(|(_, l)| *l == "This help"),
            "Views group should contain This help: {:?}",
            views.1
        );
    }

    #[test]
    fn esc_closes() {
        let mut o = HelpOverlay::new(Keymap::normal());
        assert_eq!(o.handle_key(press(KeyCode::Esc)), OverlayResult::Close);
    }

    #[test]
    fn q_closes() {
        let mut o = HelpOverlay::new(Keymap::normal());
        assert_eq!(
            o.handle_key(press(KeyCode::Char('q'))),
            OverlayResult::Close
        );
    }

    #[test]
    fn question_mark_closes() {
        let mut o = HelpOverlay::new(Keymap::normal());
        assert_eq!(
            o.handle_key(press(KeyCode::Char('?'))),
            OverlayResult::Close
        );
    }

    #[test]
    fn unrelated_keys_keep_open() {
        let mut o = HelpOverlay::new(Keymap::normal());
        assert_eq!(
            o.handle_key(press(KeyCode::Char('j'))),
            OverlayResult::KeepOpen
        );
    }

    #[test]
    fn render_border_uses_theme_overlay_border() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let o = HelpOverlay::new(Keymap::normal());
        let theme = crate::theme::Theme::terminal_default();
        let want = theme.overlay_border.as_ratatui();
        let backend = TestBackend::new(80, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| o.render_with_theme(f, f.area(), &theme))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut found = false;
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                if buf[(x, y)].symbol() == "┌" {
                    assert_eq!(buf[(x, y)].style().fg, Some(want));
                    found = true;
                }
            }
        }
        assert!(found, "expected ┌ corner glyph in rendered buffer");
    }

    #[test]
    fn format_chord_renders_ctrl_alt_arrows() {
        assert_eq!(
            format_chord(KeyCode::Char('p'), KeyModifiers::CONTROL),
            "Ctrl+p"
        );
        assert_eq!(format_chord(KeyCode::Char('1'), KeyModifiers::ALT), "Alt+1");
        assert_eq!(format_chord(KeyCode::Up, KeyModifiers::NONE), "↑");
        assert_eq!(format_chord(KeyCode::Char('?'), KeyModifiers::NONE), "?");
    }
}
