//! DaemonSets table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::apps::v1::DaemonSet;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct DaemonSetsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, DaemonSet)>,
    filter: Filter,
}

impl DaemonSetsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for DaemonSetsView {
    fn id(&self) -> &'static str {
        "daemonsets"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.daemonsets.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |_| None);
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let header = Row::new(vec![
            "",
            "NAMESPACE",
            "NAME",
            "DESIRED",
            "CURRENT",
            "READY",
            "UP-TO-DATE",
            "AVAILABLE",
            "AGE",
        ])
        .style(Style::default().fg(theme.header_fg.as_ratatui()));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, ds))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(desired_pods(ds).to_string()),
                    Cell::from(current_pods(ds).to_string()),
                    Cell::from(ready_pods(ds).to_string()),
                    Cell::from(updated_pods(ds).to_string()),
                    Cell::from(available_pods(ds).to_string()),
                    Cell::from(metadata_age(&ds.metadata)),
                ]);
                if i == self.selected {
                    row.style(
                        Style::default()
                            .fg(theme.selection_fg.as_ratatui())
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    row
                }
            })
            .collect();

        let widths = [
            Constraint::Length(2),
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(8),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" daemonsets · {} ", self.snapshot.len())),
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

fn desired_pods(ds: &DaemonSet) -> i32 {
    ds.status
        .as_ref()
        .map(|s| s.desired_number_scheduled)
        .unwrap_or(0)
}

fn current_pods(ds: &DaemonSet) -> i32 {
    ds.status
        .as_ref()
        .map(|s| s.current_number_scheduled)
        .unwrap_or(0)
}

fn ready_pods(ds: &DaemonSet) -> i32 {
    ds.status
        .as_ref()
        .map(|s| s.number_ready)
        .unwrap_or(0)
}

fn updated_pods(ds: &DaemonSet) -> i32 {
    ds.status
        .as_ref()
        .and_then(|s| s.updated_number_scheduled)
        .unwrap_or(0)
}

fn available_pods(ds: &DaemonSet) -> i32 {
    ds.status
        .as_ref()
        .and_then(|s| s.number_available)
        .unwrap_or(0)
}

fn metadata_age(m: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta) -> String {
    let Some(ts) = m.creation_timestamp.as_ref() else {
        return "?".into();
    };
    let delta = chrono::Utc::now().signed_duration_since(ts.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::DaemonSetStatus;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_ds(ns: &str, name: &str, desired: i32, ready: i32) -> DaemonSet {
        DaemonSet {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: None,
            status: Some(DaemonSetStatus {
                desired_number_scheduled: desired,
                current_number_scheduled: desired,
                number_ready: ready,
                updated_number_scheduled: Some(desired),
                number_available: Some(ready),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn renders_columns_for_three_resources_no_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = DaemonSetsView::new();
        view.snapshot = vec![
            (
                ResourceKey::namespaced("DaemonSet", "kube-system", "fluentd"),
                make_ds("kube-system", "fluentd", 3, 3),
            ),
            (
                ResourceKey::namespaced("DaemonSet", "kube-system", "node-exporter"),
                make_ds("kube-system", "node-exporter", 3, 2),
            ),
            (
                ResourceKey::namespaced("DaemonSet", "default", "datadog"),
                make_ds("default", "datadog", 5, 5),
            ),
        ];

        let theme = crate::theme::Theme::terminal_default();
        let backend = TestBackend::new(140, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
    }

    #[test]
    fn selection_marker_highlights_active_row() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = DaemonSetsView::new();
        view.snapshot = vec![(
            ResourceKey::namespaced("DaemonSet", "kube-system", "fluentd"),
            make_ds("kube-system", "fluentd", 3, 3),
        )];

        let theme = crate::theme::Theme::embedded("solarized-light").unwrap();
        let want = theme.selection_fg.as_ratatui();

        let backend = TestBackend::new(140, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();
        let mut found = false;
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                if buf[(x, y)].symbol() == "▎" {
                    found = true;
                    assert_eq!(buf[(x, y)].style().fg, Some(want));
                }
            }
        }
        assert!(found);
    }

    #[tokio::test]
    async fn refresh_loads_daemonsets_from_registry() {
        let r = StoreRegistry::new();
        r.daemonsets
            .upsert(
                ResourceKey::namespaced("DaemonSet", "kube-system", "fluentd"),
                make_ds("kube-system", "fluentd", 3, 3),
            )
            .await;
        let mut v = DaemonSetsView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
