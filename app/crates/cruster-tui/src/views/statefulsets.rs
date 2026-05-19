//! StatefulSets table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::apps::v1::StatefulSet;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct StatefulSetsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, StatefulSet)>,
    filter: Filter,
}

impl StatefulSetsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for StatefulSetsView {
    fn id(&self) -> &'static str {
        "statefulsets"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.statefulsets.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |_| None);
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let header = Row::new(vec!["", "NAMESPACE", "NAME", "READY", "AGE", "IMAGE"])
            .style(Style::default().fg(theme.header_fg.as_ratatui()));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, sts))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let (ready, desired) = ready_desired(sts);
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(format!("{ready}/{desired}")),
                    Cell::from(metadata_age(&sts.metadata)),
                    Cell::from(first_image(sts)),
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
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(30),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" statefulsets · {} ", self.snapshot.len())),
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

fn ready_desired(sts: &StatefulSet) -> (i32, i32) {
    let desired = sts.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
    let ready = sts
        .status
        .as_ref()
        .and_then(|s| s.ready_replicas)
        .unwrap_or(0);
    (ready, desired)
}

fn first_image(sts: &StatefulSet) -> String {
    sts.spec
        .as_ref()
        .and_then(|s| s.template.spec.as_ref())
        .and_then(|ps| ps.containers.first())
        .and_then(|c| c.image.clone())
        .unwrap_or_else(|| "-".into())
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
    use k8s_openapi::api::apps::v1::{StatefulSetSpec, StatefulSetStatus};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_sts(ns: &str, name: &str, ready: i32, desired: i32) -> StatefulSet {
        StatefulSet {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: Some(StatefulSetSpec {
                replicas: Some(desired),
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            name: "main".into(),
                            image: Some("postgres:15".into()),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }),
            status: Some(StatefulSetStatus {
                ready_replicas: Some(ready),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn renders_columns_for_three_resources_no_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = StatefulSetsView::new();
        view.snapshot = vec![
            (
                ResourceKey::namespaced("StatefulSet", "default", "pg"),
                make_sts("default", "pg", 2, 3),
            ),
            (
                ResourceKey::namespaced("StatefulSet", "default", "redis"),
                make_sts("default", "redis", 1, 1),
            ),
            (
                ResourceKey::namespaced("StatefulSet", "staging", "kafka"),
                make_sts("staging", "kafka", 3, 3),
            ),
        ];

        let theme = crate::theme::Theme::terminal_default();
        let backend = TestBackend::new(120, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
    }

    #[test]
    fn selection_marker_highlights_active_row() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = StatefulSetsView::new();
        view.snapshot = vec![(
            ResourceKey::namespaced("StatefulSet", "default", "pg"),
            make_sts("default", "pg", 1, 2),
        )];

        let theme = crate::theme::Theme::embedded("solarized-light").unwrap();
        let want = theme.selection_fg.as_ratatui();

        let backend = TestBackend::new(120, 5);
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
    async fn refresh_loads_statefulsets_from_registry() {
        let r = StoreRegistry::new();
        r.statefulsets
            .upsert(
                ResourceKey::namespaced("StatefulSet", "default", "pg"),
                make_sts("default", "pg", 1, 1),
            )
            .await;
        let mut v = StatefulSetsView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
