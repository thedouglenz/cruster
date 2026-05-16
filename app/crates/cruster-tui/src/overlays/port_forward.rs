//! Port-forward modal: prompts the user for `local:remote` mapping.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::overlay::{Overlay, OverlayResult};

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

        let title = format!(" port-forward pod/{} ", self.pod_key.name);
        let input = Paragraph::new(format!("local:remote → {}_", self.buffer))
            .block(Block::default().borders(Borders::ALL).title(title));
        frame.render_widget(input, chunks[0]);

        let hint = Paragraph::new("enter applies · esc cancels")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[1]);
    }

    fn port_forward_payload(&self) -> Option<(ResourceKey, String)> {
        Some((self.pod_key.clone(), self.buffer.clone()))
    }
}
