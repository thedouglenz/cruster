//! Relationships overlay: pick a related resource to jump to.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_kube::relationships::Related;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::overlay::{Overlay, OverlayResult};

pub struct RelationshipsOverlay {
    related: Vec<Related>,
    selected: usize,
    title: String,
}

impl RelationshipsOverlay {
    pub fn new(title: impl Into<String>, related: Vec<Related>) -> Self {
        Self {
            related,
            selected: 0,
            title: title.into(),
        }
    }
}

impl Overlay for RelationshipsOverlay {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => {
                let Some(r) = self.related.get(self.selected) else {
                    return OverlayResult::Close;
                };
                let view_id = match r.key.kind.as_str() {
                    "Pod" => "pods",
                    "Deployment" => "deployments",
                    "Service" => "services",
                    "Node" => "nodes",
                    "Event" => "events",
                    "ConfigMap" => "configmaps",
                    "Secret" => "secrets",
                    "Namespace" => "namespaces",
                    "ReplicaSet" => "deployments",
                    _ => return OverlayResult::Close,
                };
                OverlayResult::SwitchView(view_id.into())
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                OverlayResult::KeepOpen
            }
            KeyCode::Down => {
                self.selected = self
                    .selected
                    .saturating_add(1)
                    .min(self.related.len().saturating_sub(1));
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
        let header = Paragraph::new(format!(
            "related to {} ({})",
            self.title,
            self.related.len()
        ))
        .block(Block::default().borders(Borders::ALL).title(" Relationships "));
        frame.render_widget(header, chunks[0]);
        let items: Vec<ListItem> = self
            .related
            .iter()
            .map(|r| {
                ListItem::new(format!(
                    "[{:12}]  {}/{}",
                    r.kind.label(),
                    r.key.kind,
                    r.key.name
                ))
            })
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
