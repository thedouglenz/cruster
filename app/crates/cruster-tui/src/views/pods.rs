//! Pods table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Pod;
use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};

use crate::app::LoopState;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct PodsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Pod)>,
}

impl PodsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn row_count(&self) -> usize {
        self.snapshot.len()
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let n = self.row_count();
        if n == 0 {
            self.selected = 0;
            return;
        }
        let max = n.saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    pub fn move_to_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_to_bottom(&mut self) {
        self.selected = self.row_count().saturating_sub(1);
    }
}

#[async_trait]
impl ResourceView for PodsView {
    fn id(&self) -> &'static str {
        "pods"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        self.snapshot = registry.pods.snapshot().await;
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "STATUS", "READY", "RESTARTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, pod)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(pod_phase(pod)),
                    Cell::from(pod_ready(pod)),
                    Cell::from(pod_restarts(pod).to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " pods ({}) — j/k move · :kind switch · q quit ",
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
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Char('g') | KeyCode::Home => self.move_to_top(),
            KeyCode::Char('G') | KeyCode::End => self.move_to_bottom(),
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

fn pod_phase(pod: &Pod) -> String {
    pod.status
        .as_ref()
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "?".to_string())
}

fn pod_ready(pod: &Pod) -> String {
    let containers = pod
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|c| c.as_slice())
        .unwrap_or(&[]);
    let total = containers.len();
    let ready = containers.iter().filter(|c| c.ready).count();
    format!("{ready}/{total}")
}

fn pod_restarts(pod: &Pod) -> i32 {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruster_kube::StoreRegistry;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_pod_entry(namespace: &str, name: &str) -> (ResourceKey, Pod) {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(namespace.into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let k = ResourceKey::namespaced("Pod", namespace, name);
        (k, pod)
    }

    #[test]
    fn move_down_on_empty_stays_at_zero() {
        let mut v = PodsView::new();
        v.move_down();
        assert_eq!(v.selected(), 0);
    }

    #[test]
    fn move_down_clamps_to_last_row() {
        let mut v = PodsView::new();
        for i in 0..3 {
            v.snapshot.push(make_pod_entry("default", &format!("p{i}")));
        }
        for _ in 0..10 {
            v.move_down();
        }
        assert_eq!(v.selected(), 2);
    }

    #[test]
    fn move_up_on_zero_stays_at_zero() {
        let mut v = PodsView::new();
        v.move_up();
        assert_eq!(v.selected(), 0);
    }

    #[test]
    fn move_to_bottom_on_empty_is_zero() {
        let mut v = PodsView::new();
        v.move_to_bottom();
        assert_eq!(v.selected(), 0);
    }

    #[tokio::test]
    async fn refresh_loads_snapshot_from_registry() {
        let registry = StoreRegistry::new();
        let (key, pod) = make_pod_entry("default", "nginx");
        registry.pods.upsert(key, pod).await;

        let mut view = PodsView::new();
        view.refresh(&registry).await;
        assert_eq!(view.snapshot.len(), 1);
    }

    #[tokio::test]
    async fn refresh_clamps_selection_when_rows_removed() {
        let registry = StoreRegistry::new();
        for i in 0..5 {
            let (k, p) = make_pod_entry("default", &format!("p{i}"));
            registry.pods.upsert(k, p).await;
        }

        let mut view = PodsView::new();
        view.refresh(&registry).await;
        view.selected = 4;

        registry.pods.replace_all(std::iter::empty()).await;
        let (k, p) = make_pod_entry("default", "only");
        registry.pods.upsert(k, p).await;
        view.refresh(&registry).await;
        assert_eq!(view.selected, 0);
    }
}
