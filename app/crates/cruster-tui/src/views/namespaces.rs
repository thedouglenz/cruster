//! Namespaces table view (cluster-scoped).

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Namespace;
use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::view::ResourceView;
use crate::views::configmaps::metadata_age;

#[derive(Debug, Default)]
pub struct NamespacesView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Namespace)>,
    filter: Filter,
}

impl NamespacesView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for NamespacesView {
    fn id(&self) -> &'static str {
        "namespaces"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.namespaces.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |n| {
            n.status.as_ref().and_then(|s| s.phase.clone())
        });
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header =
            Row::new(vec!["", "NAME", "STATUS", "AGE"]).style(Style::default().fg(Color::DarkGray));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, ns))| {
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(key.name.clone()),
                    Cell::from(namespace_status(ns)),
                    Cell::from(metadata_age(&ns.metadata)),
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
            Constraint::Min(20),
            Constraint::Length(12),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" namespaces · {} ", self.snapshot.len())),
        );

        frame.render_widget(table, area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if !self.snapshot.is_empty() => {
                let max = self.snapshot.len() - 1;
                self.selected = (self.selected + 1).min(max);
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

fn namespace_status(n: &Namespace) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Active".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::NamespaceStatus;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn namespace_status_defaults_to_active() {
        let n = Namespace {
            metadata: ObjectMeta {
                name: Some("default".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(namespace_status(&n), "Active");
    }

    #[test]
    fn namespace_status_uses_phase_when_present() {
        let n = Namespace {
            metadata: ObjectMeta {
                name: Some("foo".into()),
                ..Default::default()
            },
            status: Some(NamespaceStatus {
                phase: Some("Terminating".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(namespace_status(&n), "Terminating");
    }
}
