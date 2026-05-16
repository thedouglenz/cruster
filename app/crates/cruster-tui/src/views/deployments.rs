//! Deployments table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::apps::v1::Deployment;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::app::LoopState;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct DeploymentsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Deployment)>,
}

impl DeploymentsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for DeploymentsView {
    fn id(&self) -> &'static str {
        "deployments"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        self.snapshot = registry.deployments.snapshot().await;
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec![
            "NAMESPACE",
            "NAME",
            "READY",
            "UP-TO-DATE",
            "AVAILABLE",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, dep)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let (ready, desired) = ready_desired(dep);
                let updated = updated_replicas(dep);
                let available = available_replicas(dep);
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(format!("{ready}/{desired}")),
                    Cell::from(updated.to_string()),
                    Cell::from(available.to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " deployments ({}) — j/k move · :kind switch · q quit ",
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

fn ready_desired(dep: &Deployment) -> (i32, i32) {
    let desired = dep.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
    let ready = dep
        .status
        .as_ref()
        .and_then(|s| s.ready_replicas)
        .unwrap_or(0);
    (ready, desired)
}

fn updated_replicas(dep: &Deployment) -> i32 {
    dep.status
        .as_ref()
        .and_then(|s| s.updated_replicas)
        .unwrap_or(0)
}

fn available_replicas(dep: &Deployment) -> i32 {
    dep.status
        .as_ref()
        .and_then(|s| s.available_replicas)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{DeploymentSpec, DeploymentStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_dep(ns: &str, name: &str, ready: i32, desired: i32) -> Deployment {
        Deployment {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(desired),
                ..Default::default()
            }),
            status: Some(DeploymentStatus {
                ready_replicas: Some(ready),
                updated_replicas: Some(desired),
                available_replicas: Some(ready),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn ready_desired_extracts_from_spec_and_status() {
        let d = make_dep("default", "web", 2, 3);
        assert_eq!(ready_desired(&d), (2, 3));
    }

    #[test]
    fn ready_desired_defaults_to_zero_zero_when_missing() {
        let d = Deployment::default();
        assert_eq!(ready_desired(&d), (0, 0));
    }

    #[tokio::test]
    async fn refresh_loads_deployments_from_registry() {
        let r = StoreRegistry::new();
        r.deployments
            .upsert(
                ResourceKey::namespaced("Deployment", "default", "web"),
                make_dep("default", "web", 1, 1),
            )
            .await;
        let mut v = DeploymentsView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
