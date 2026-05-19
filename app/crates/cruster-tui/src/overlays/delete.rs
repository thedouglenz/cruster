//! Delete modal: confirms a destructive `kubectl delete` against the
//! selected resource. Lets the user pick a propagation policy and
//! optionally toggle `--grace-period=0 --force`.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::actions::delete::PropagationPolicy;
use crate::overlay::{Overlay, OverlayResult};
use crate::overlays::chrome::centered_rect;
use crate::theme::Theme;

pub struct DeleteOverlay {
    key: ResourceKey,
    policy: PropagationPolicy,
    force: bool,
}

impl DeleteOverlay {
    pub fn new(key: ResourceKey) -> Self {
        Self {
            key,
            policy: PropagationPolicy::Background,
            force: false,
        }
    }

    /// Test-friendly accessors.
    pub fn policy(&self) -> PropagationPolicy {
        self.policy
    }
    pub fn force(&self) -> bool {
        self.force
    }

    fn cycle_left(&mut self) {
        self.policy = match self.policy {
            PropagationPolicy::Background => PropagationPolicy::Orphan,
            PropagationPolicy::Foreground => PropagationPolicy::Background,
            PropagationPolicy::Orphan => PropagationPolicy::Foreground,
        };
    }

    fn cycle_right(&mut self) {
        self.policy = match self.policy {
            PropagationPolicy::Background => PropagationPolicy::Foreground,
            PropagationPolicy::Foreground => PropagationPolicy::Orphan,
            PropagationPolicy::Orphan => PropagationPolicy::Background,
        };
    }
}

impl Overlay for DeleteOverlay {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => OverlayResult::Invoke("delete-submit".into()),
            KeyCode::Char('b') | KeyCode::Char('B') => {
                self.policy = PropagationPolicy::Background;
                OverlayResult::KeepOpen
            }
            KeyCode::Char('f') => {
                self.policy = PropagationPolicy::Foreground;
                OverlayResult::KeepOpen
            }
            KeyCode::Char('F') => {
                self.force = !self.force;
                OverlayResult::KeepOpen
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                self.policy = PropagationPolicy::Orphan;
                OverlayResult::KeepOpen
            }
            KeyCode::Left => {
                self.cycle_left();
                OverlayResult::KeepOpen
            }
            KeyCode::Right | KeyCode::Tab => {
                self.cycle_right();
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let theme = Theme::terminal_default();
        self.render_with_theme(frame, area, &theme);
    }

    fn delete_payload(&self) -> Option<(ResourceKey, PropagationPolicy, bool)> {
        Some((self.key.clone(), self.policy, self.force))
    }
}

impl DeleteOverlay {
    /// Render with an explicit theme. The trait method delegates with
    /// the default theme so existing call sites don't have to be
    /// reworked; the App can call this directly to get themed colors.
    pub fn render_with_theme(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let Some(rect) = centered_rect(area, 30, 7, 60, 9) else {
            return;
        };
        // Wipe whatever's underneath so the modal reads cleanly.
        frame.render_widget(Clear, rect);

        let title = match self.key.namespace.as_deref() {
            Some(ns) => format!(" Delete {}/{} in {} ", self.key.kind, self.key.name, ns),
            None => format!(" Delete {}/{} ", self.key.kind, self.key.name),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.overlay_border.as_ratatui()))
            .title(title)
            .title_style(
                Style::default()
                    .fg(theme.status.failed.as_ratatui())
                    .add_modifier(Modifier::BOLD),
            );
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(inner);

        // Row 1: propagation picker. Selected option is bracketed +
        // bold; others render in muted_fg so the active choice pops.
        let muted = Style::default().fg(theme.muted_fg.as_ratatui());
        let active = Style::default()
            .fg(theme.selection_fg.as_ratatui())
            .add_modifier(Modifier::BOLD);
        let mut spans: Vec<Span<'static>> = vec![Span::styled("  Propagation:  ", muted)];
        for p in [
            PropagationPolicy::Background,
            PropagationPolicy::Foreground,
            PropagationPolicy::Orphan,
        ] {
            if p == self.policy {
                spans.push(Span::styled(format!("[ {} ]", p.label()), active));
            } else {
                spans.push(Span::styled(format!("  {}  ", p.label()), muted));
            }
            spans.push(Span::raw(" "));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

        // Row 2: force checkbox.
        let force_glyph = if self.force { "[x]" } else { "[ ]" };
        let force_style = if self.force {
            Style::default()
                .fg(theme.status.failed.as_ratatui())
                .add_modifier(Modifier::BOLD)
        } else {
            muted
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                format!("  {} Force (--grace-period=0)", force_glyph),
                force_style,
            )])),
            chunks[1],
        );

        // Row 4: hint footer.
        frame.render_widget(
            Paragraph::new("  enter confirms · b/f/o cycle · F toggle force · esc cancels")
                .style(muted),
            chunks[4],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent {
            code: c,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn overlay() -> DeleteOverlay {
        DeleteOverlay::new(ResourceKey::namespaced("Pod", "default", "nginx"))
    }

    #[test]
    fn default_policy_is_background_force_off() {
        let o = overlay();
        assert_eq!(o.policy(), PropagationPolicy::Background);
        assert!(!o.force());
    }

    #[test]
    fn b_f_o_select_named_policy() {
        let mut o = overlay();
        let _ = o.handle_key(key('f'));
        assert_eq!(o.policy(), PropagationPolicy::Foreground);
        let _ = o.handle_key(key('o'));
        assert_eq!(o.policy(), PropagationPolicy::Orphan);
        let _ = o.handle_key(key('b'));
        assert_eq!(o.policy(), PropagationPolicy::Background);
    }

    #[test]
    fn capital_f_toggles_force() {
        let mut o = overlay();
        assert!(!o.force());
        let _ = o.handle_key(key('F'));
        assert!(o.force());
        let _ = o.handle_key(key('F'));
        assert!(!o.force());
    }

    #[test]
    fn capital_f_does_not_change_policy() {
        let mut o = overlay();
        // Default is Background; capital F should not flip it to
        // Foreground (lowercase f selects Foreground; capital F is
        // the force toggle).
        let _ = o.handle_key(key('F'));
        assert_eq!(o.policy(), PropagationPolicy::Background);
    }

    #[test]
    fn arrows_cycle_policy_both_directions() {
        let mut o = overlay();
        let _ = o.handle_key(code(KeyCode::Right));
        assert_eq!(o.policy(), PropagationPolicy::Foreground);
        let _ = o.handle_key(code(KeyCode::Right));
        assert_eq!(o.policy(), PropagationPolicy::Orphan);
        let _ = o.handle_key(code(KeyCode::Right));
        assert_eq!(o.policy(), PropagationPolicy::Background);
        let _ = o.handle_key(code(KeyCode::Left));
        assert_eq!(o.policy(), PropagationPolicy::Orphan);
    }

    #[test]
    fn enter_returns_invoke_delete_submit() {
        let mut o = overlay();
        let r = o.handle_key(code(KeyCode::Enter));
        assert_eq!(r, OverlayResult::Invoke("delete-submit".into()));
    }

    #[test]
    fn esc_returns_close() {
        let mut o = overlay();
        let r = o.handle_key(code(KeyCode::Esc));
        assert_eq!(r, OverlayResult::Close);
    }

    #[test]
    fn delete_payload_returns_current_choices() {
        let mut o = overlay();
        let _ = o.handle_key(key('f'));
        let _ = o.handle_key(key('F'));
        let (k, p, force) = o.delete_payload().unwrap();
        assert_eq!(k.kind, "Pod");
        assert_eq!(k.name, "nginx");
        assert_eq!(p, PropagationPolicy::Foreground);
        assert!(force);
    }

    #[test]
    fn render_border_uses_theme_overlay_border() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let o = overlay();
        let theme = crate::theme::Theme::terminal_default();
        let want = theme.overlay_border.as_ratatui();
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| o.render_with_theme(f, f.area(), &theme))
            .unwrap();
        let buf = terminal.backend().buffer();
        // Hunt for the top-left corner glyph ┌ — its fg must match.
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
    fn key_release_is_ignored() {
        let mut o = overlay();
        let release = KeyEvent {
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        let _ = o.handle_key(release);
        assert_eq!(o.policy(), PropagationPolicy::Background);
    }
}
