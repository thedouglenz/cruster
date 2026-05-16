//! Nodes table view (cluster-scoped — no NAMESPACE column).

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Node;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct NodesView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Node)>,
    filter: Filter,
}

impl NodesView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for NodesView {
    fn id(&self) -> &'static str {
        "nodes"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.nodes.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |n| Some(node_status(n)));
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAME", "STATUS", "ROLES", "VERSION", "OS-IMAGE"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, node)| {
                Row::new(vec![
                    Cell::from(key.name.clone()),
                    Cell::from(node_status(node)),
                    Cell::from(node_roles(node)),
                    Cell::from(node_version(node)),
                    Cell::from(node_os_image(node)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(30),
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(14),
            Constraint::Min(20),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " nodes ({}) — j/k move · :kind switch · q quit ",
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

    fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }
}

fn node_status(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .and_then(|cs| cs.iter().find(|c| c.type_ == "Ready"))
        .map(|c| {
            if c.status == "True" {
                "Ready".into()
            } else {
                "NotReady".into()
            }
        })
        .unwrap_or_else(|| "?".into())
}

fn node_roles(n: &Node) -> String {
    let s = n
        .metadata
        .labels
        .as_ref()
        .map(|labels| {
            labels
                .keys()
                .filter_map(|k| k.strip_prefix("node-role.kubernetes.io/"))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    if s.is_empty() {
        "<none>".into()
    } else {
        s
    }
}

fn node_version(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.kubelet_version.clone())
        .unwrap_or_else(|| "?".into())
}

fn node_os_image(n: &Node) -> String {
    n.status
        .as_ref()
        .and_then(|s| s.node_info.as_ref())
        .map(|info| info.os_image.clone())
        .unwrap_or_else(|| "?".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{NodeCondition, NodeStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_node_with_condition(name: &str, condition_type: &str, status: &str) -> Node {
        Node {
            metadata: ObjectMeta {
                name: Some(name.into()),
                ..Default::default()
            },
            status: Some(NodeStatus {
                conditions: Some(vec![NodeCondition {
                    type_: condition_type.into(),
                    status: status.into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn node_status_returns_ready_for_true_ready_condition() {
        let n = make_node_with_condition("n1", "Ready", "True");
        assert_eq!(node_status(&n), "Ready");
    }

    #[test]
    fn node_status_returns_notready_for_false_ready_condition() {
        let n = make_node_with_condition("n1", "Ready", "False");
        assert_eq!(node_status(&n), "NotReady");
    }

    #[test]
    fn node_roles_returns_none_when_no_role_labels() {
        let n = Node {
            metadata: ObjectMeta {
                name: Some("n1".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(node_roles(&n), "<none>");
    }
}
