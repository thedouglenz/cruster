//! Events table view (sorted by lastTimestamp desc).

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Event;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct EventsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Event)>,
    filter: Filter,
}

impl EventsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for EventsView {
    fn id(&self) -> &'static str {
        "events"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let mut snap = registry.events.snapshot().await;
        snap.sort_by(|a, b| event_time(&b.1).cmp(&event_time(&a.1)));
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |e| e.type_.clone());
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec![
            "",
            "NAMESPACE",
            "LAST SEEN",
            "TYPE",
            "REASON",
            "OBJECT",
            "MESSAGE",
        ])
        .style(Style::default().fg(Color::DarkGray));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, e))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(ns.to_string()),
                    Cell::from(event_age_str(e)),
                    Cell::from(e.type_.clone().unwrap_or_default()),
                    Cell::from(e.reason.clone().unwrap_or_default()),
                    Cell::from(event_object(e)),
                    Cell::from(e.message.clone().unwrap_or_default()),
                ]);
                if i == self.selected {
                    row.style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    row
                }
            })
            .collect();

        let widths = [
            Constraint::Length(2),
            Constraint::Length(16),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(20),
            Constraint::Length(30),
            Constraint::Min(20),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" events · {} ", self.snapshot.len())),
        );

        frame.render_widget(table, area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.snapshot.is_empty() {
                    let max = self.snapshot.len() - 1;
                    self.selected = (self.selected + 1).min(max);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.selected = self.snapshot.len().saturating_sub(1);
            }
            _ => {}
        }
        LoopState::Continue
    }

    fn selected_yaml(&self) -> Option<(String, String)> {
        let (key, obj) = self.snapshot.get(self.selected)?;
        let yaml = serde_yaml::to_string(obj).ok()?;
        Some((key.to_string(), yaml))
    }

    fn selected_key(&self) -> Option<ResourceKey> {
        self.snapshot.get(self.selected).map(|(k, _)| k.clone())
    }

    fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }
}

fn event_time(e: &Event) -> Option<Time> {
    e.last_timestamp
        .clone()
        .or_else(|| e.event_time.clone().map(|mt| Time(mt.0)))
}

fn event_age_str(e: &Event) -> String {
    match event_time(e) {
        Some(t) => human_age(t.0),
        None => "?".into(),
    }
}

/// Compact human-readable age of a timestamp, k8s/kubectl-style:
/// `45s`, `12m`, `3h`, `5d`.
pub fn human_age(ts: chrono::DateTime<chrono::Utc>) -> String {
    let delta = chrono::Utc::now().signed_duration_since(ts);
    let secs = delta.num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn event_object(e: &Event) -> String {
    let kind = e.involved_object.kind.as_deref().unwrap_or("?");
    match &e.involved_object.name {
        Some(n) => format!("{kind}/{n}"),
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn human_age_seconds() {
        let ts = chrono::Utc::now() - Duration::seconds(30);
        assert_eq!(human_age(ts), "30s");
    }

    #[test]
    fn human_age_minutes() {
        let ts = chrono::Utc::now() - Duration::minutes(5);
        assert_eq!(human_age(ts), "5m");
    }

    #[test]
    fn human_age_hours() {
        let ts = chrono::Utc::now() - Duration::hours(2);
        assert_eq!(human_age(ts), "2h");
    }

    #[test]
    fn human_age_days() {
        let ts = chrono::Utc::now() - Duration::days(3);
        assert_eq!(human_age(ts), "3d");
    }

    #[test]
    fn human_age_future_clamps_to_zero() {
        let ts = chrono::Utc::now() + Duration::hours(1);
        assert_eq!(human_age(ts), "0s");
    }
}
