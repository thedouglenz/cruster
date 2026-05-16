//! Namespaces table view (cluster-scoped).

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Namespace;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::app::LoopState;
use crate::view::ResourceView;
use crate::views::configmaps::metadata_age;

#[derive(Debug, Default)]
pub struct NamespacesView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Namespace)>,
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
        self.snapshot = registry.namespaces.snapshot().await;
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAME", "STATUS", "AGE"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, ns)| {
                Row::new(vec![
                    Cell::from(key.name.clone()),
                    Cell::from(namespace_status(ns)),
                    Cell::from(metadata_age(&ns.metadata)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Min(20),
            Constraint::Length(12),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " namespaces ({}) — j/k move · :kind switch · q quit ",
                self.snapshot.len()
            )))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if !self.snapshot.is_empty() {
            state.select(Some(self.selected.min(self.snapshot.len() - 1)));
        }

        frame.render_stateful_widget(table, area, &mut state);
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
