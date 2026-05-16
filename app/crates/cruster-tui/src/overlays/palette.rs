//! Command palette: fuzzy-match over actions + kinds.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::overlay::{Overlay, OverlayResult};

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub id: String,
    pub label: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Action,
    View,
}

pub struct Palette {
    entries: Vec<PaletteEntry>,
    query: String,
    selected: usize,
    matcher: Matcher,
}

impl Palette {
    pub fn new(entries: Vec<PaletteEntry>) -> Self {
        Self {
            entries,
            query: String::new(),
            selected: 0,
            matcher: Matcher::default(),
        }
    }

    fn filtered(&mut self) -> Vec<(&PaletteEntry, u32)> {
        if self.query.is_empty() {
            return self.entries.iter().map(|e| (e, 0)).collect();
        }
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(&PaletteEntry, u32)> = self
            .entries
            .iter()
            .filter_map(|e| {
                buf.clear();
                let score = pattern.score(Utf32Str::new(&e.label, &mut buf), &mut self.matcher)?;
                Some((e, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored
    }
}

impl Overlay for Palette {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => {
                let selected = self.selected;
                let filtered = self.filtered();
                let Some((entry, _)) = filtered.get(selected) else {
                    return OverlayResult::Close;
                };
                match entry.kind {
                    EntryKind::Action => OverlayResult::Invoke(entry.id.clone()),
                    EntryKind::View => OverlayResult::SwitchView(entry.id.clone()),
                }
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                OverlayResult::KeepOpen
            }
            KeyCode::Down => {
                self.selected = self.selected.saturating_add(1);
                OverlayResult::KeepOpen
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
                OverlayResult::KeepOpen
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.selected = 0;
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let w = area.width.saturating_sub(20).min(80);
        let h = area.height.saturating_sub(6).min(20);
        if w < 20 || h < 6 {
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
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(rect);

        let query = Paragraph::new(format!("> {}", self.query))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Command Palette "),
            )
            .style(Style::default());
        frame.render_widget(query, chunks[0]);

        // For rendering we filter locally (the matcher is in &mut self
        // but render is &self; for v1 we do a simple substring filter
        // for the visible list. The Enter path uses the proper fuzzy
        // scoring which mutates self.matcher.)
        let visible: Vec<&PaletteEntry> = self
            .entries
            .iter()
            .filter(|e| {
                self.query.is_empty() || e.label.to_lowercase().contains(&self.query.to_lowercase())
            })
            .collect();
        let items: Vec<ListItem> = visible
            .iter()
            .map(|e| ListItem::new(format!("{:8}  {}", entry_tag(e.kind), e.label)))
            .collect();

        let mut state = ListState::default();
        if !items.is_empty() {
            state.select(Some(self.selected.min(items.len() - 1)));
        }
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_stateful_widget(list, chunks[1], &mut state);
    }
}

fn entry_tag(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Action => "[action]",
        EntryKind::View => "[view]",
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

    fn p() -> Palette {
        Palette::new(vec![
            PaletteEntry {
                id: "describe".into(),
                label: "Describe".into(),
                kind: EntryKind::Action,
            },
            PaletteEntry {
                id: "deployments".into(),
                label: "Deployments".into(),
                kind: EntryKind::View,
            },
        ])
    }

    #[test]
    fn esc_closes() {
        let mut pal = p();
        assert_eq!(pal.handle_key(press(KeyCode::Esc)), OverlayResult::Close);
    }

    #[test]
    fn enter_invokes_action() {
        let mut pal = p();
        assert_eq!(
            pal.handle_key(press(KeyCode::Enter)),
            OverlayResult::Invoke("describe".into())
        );
    }

    #[test]
    fn down_then_enter_invokes_view_switch() {
        let mut pal = p();
        pal.handle_key(press(KeyCode::Down));
        assert_eq!(
            pal.handle_key(press(KeyCode::Enter)),
            OverlayResult::SwitchView("deployments".into())
        );
    }

    #[test]
    fn typing_filters_and_scores() {
        let mut pal = p();
        for c in "depl".chars() {
            pal.handle_key(press(KeyCode::Char(c)));
        }
        let filtered = pal.filtered();
        assert!(!filtered.is_empty());
        assert_eq!(filtered[0].0.label, "Deployments");
    }

    #[test]
    fn backspace_removes_char() {
        let mut pal = p();
        for c in "dep".chars() {
            pal.handle_key(press(KeyCode::Char(c)));
        }
        pal.handle_key(press(KeyCode::Backspace));
        assert_eq!(pal.query, "de");
    }
}
