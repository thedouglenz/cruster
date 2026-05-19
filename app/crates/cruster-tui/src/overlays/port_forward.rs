//! Port-forward modal: prompts the user for `local:remote` mapping.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::overlay::{Overlay, OverlayResult};
use crate::theme::Theme;

pub struct PortForwardOverlay {
    pod_key: ResourceKey,
    buffer: String,
}

impl PortForwardOverlay {
    pub fn new(pod_key: ResourceKey) -> Self {
        Self {
            pod_key,
            buffer: String::new(),
        }
    }
}

impl Overlay for PortForwardOverlay {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => OverlayResult::Invoke("port-forward-submit".into()),
            KeyCode::Backspace => {
                self.buffer.pop();
                OverlayResult::KeepOpen
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let w = area.width.saturating_sub(20).min(60);
        let h: u16 = 5;
        if w < 20 || h > area.height {
            return;
        }
        let x = area.x + (area.width - w) / 2;
        let y = area.y + (area.height - h) / 2;
        let rect = Rect {
            x,
            y,
            width: w,
            height: h,
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(2)])
            .split(rect);

        let theme = Theme::terminal_default();
        let title = format!(" port-forward pod/{} ", self.pod_key.name);
        let input = Paragraph::new(format!("local:remote → {}_", self.buffer)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.overlay_border.as_ratatui()))
                .title(title),
        );
        frame.render_widget(input, chunks[0]);

        let hint = Paragraph::new("enter applies · esc cancels")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[1]);
    }

    fn port_forward_payload(&self) -> Option<(ResourceKey, String)> {
        Some((self.pod_key.clone(), self.buffer.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn render_border_uses_theme_overlay_border() {
        let o = PortForwardOverlay::new(ResourceKey::namespaced("Pod", "default", "nginx"));
        let theme = Theme::terminal_default();
        let want = theme.overlay_border.as_ratatui();
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| o.render(f, f.area())).unwrap();
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
}
