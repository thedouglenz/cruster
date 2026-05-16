//! Describe action: shows a YAML dump of the currently selected
//! resource. (Phase 2B replaces with a structured describe.)

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Default)]
pub struct DescribePane {
    open: bool,
    title: String,
    content: String,
    scroll: u16,
}

impl DescribePane {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, title: impl Into<String>, content: impl Into<String>) {
        self.title = title.into();
        self.content = content.into();
        self.scroll = 0;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.title.clear();
        self.content.clear();
        self.scroll = 0;
    }

    /// Returns `true` if the key was consumed by the pane.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if !self.open || key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.close();
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                true
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                true
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                true
            }
            _ => true, // swallow other keys while open
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        if !self.open {
            return;
        }
        let focus_tag = if focused { " ◉" } else { "" };
        let block = Block::default().borders(Borders::ALL).title(format!(
            " describe: {}{} — esc closes ",
            self.title, focus_tag
        ));
        let para = Paragraph::new(self.content.clone())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        frame.render_widget(para, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn open_then_close() {
        let mut p = DescribePane::new();
        p.open("Pod/default/nginx", "yaml here");
        assert!(p.is_open());
        p.close();
        assert!(!p.is_open());
    }

    #[test]
    fn esc_closes() {
        let mut p = DescribePane::new();
        p.open("x", "y");
        assert!(p.handle_key(press(KeyCode::Esc)));
        assert!(!p.is_open());
    }

    #[test]
    fn scroll_advances_with_j() {
        let mut p = DescribePane::new();
        p.open("x", "y");
        p.handle_key(press(KeyCode::Char('j')));
        p.handle_key(press(KeyCode::Char('j')));
        assert_eq!(p.scroll, 2);
    }

    #[test]
    fn scroll_does_not_underflow() {
        let mut p = DescribePane::new();
        p.open("x", "y");
        p.handle_key(press(KeyCode::Char('k')));
        assert_eq!(p.scroll, 0);
    }
}
