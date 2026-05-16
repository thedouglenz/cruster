//! Services table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Service;
use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

use crate::app::LoopState;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct ServicesView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Service)>,
}

impl ServicesView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for ServicesView {
    fn id(&self) -> &'static str {
        "services"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        self.snapshot = registry.services.snapshot().await;
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "TYPE", "CLUSTER-IP", "PORTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, svc)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(svc_type(svc)),
                    Cell::from(svc_cluster_ip(svc)),
                    Cell::from(svc_ports(svc)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Min(20),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " services ({}) — j/k move · :kind switch · q quit ",
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
}

fn svc_type(s: &Service) -> String {
    s.spec
        .as_ref()
        .and_then(|sp| sp.type_.clone())
        .unwrap_or_else(|| "?".into())
}

fn svc_cluster_ip(s: &Service) -> String {
    s.spec
        .as_ref()
        .and_then(|sp| sp.cluster_ip.clone())
        .unwrap_or_else(|| "-".into())
}

fn svc_ports(s: &Service) -> String {
    s.spec
        .as_ref()
        .and_then(|sp| sp.ports.as_ref())
        .map(|ps| {
            ps.iter()
                .map(|p| match p.protocol.as_deref() {
                    Some(proto) => format!("{}/{}", p.port, proto),
                    None => format!("{}/TCP", p.port),
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| "-".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{ServicePort, ServiceSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_svc(ns: &str, name: &str, type_: &str, cluster_ip: &str, ports: Vec<(i32, &str)>) -> Service {
        Service {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                type_: Some(type_.into()),
                cluster_ip: Some(cluster_ip.into()),
                ports: Some(
                    ports
                        .into_iter()
                        .map(|(p, proto)| ServicePort {
                            port: p,
                            protocol: Some(proto.into()),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn svc_ports_joins_multiple_ports_with_comma() {
        let s = make_svc("default", "web", "ClusterIP", "10.0.0.1", vec![(80, "TCP"), (443, "TCP")]);
        assert_eq!(svc_ports(&s), "80/TCP,443/TCP");
    }

    #[test]
    fn svc_ports_defaults_protocol_to_tcp_when_missing() {
        let mut s = make_svc("default", "web", "ClusterIP", "10.0.0.1", vec![]);
        s.spec.as_mut().unwrap().ports = Some(vec![ServicePort {
            port: 8080,
            protocol: None,
            ..Default::default()
        }]);
        assert_eq!(svc_ports(&s), "8080/TCP");
    }

    #[tokio::test]
    async fn refresh_loads_services_from_registry() {
        let r = StoreRegistry::new();
        r.services
            .upsert(
                ResourceKey::namespaced("Service", "default", "web"),
                make_svc("default", "web", "ClusterIP", "10.0.0.1", vec![(80, "TCP")]),
            )
            .await;
        let mut v = ServicesView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
