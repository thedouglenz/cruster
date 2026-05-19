//! Pods table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Pod;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct PodsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Pod)>,
    filter: Filter,
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
        let snap = registry.pods.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |p| Some(pod_phase(p)));
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let header = Row::new(vec!["", "NAMESPACE", "NAME", "STATUS", "READY", "RESTARTS"])
            .style(Style::default().fg(theme.header_fg.as_ratatui()));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, pod))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let marker = if i == self.selected { "▎" } else { " " };
                let ready_str = pod_ready(pod);
                let ready_style = super::ready_cell_style(&ready_str, theme);
                let phase = pod_phase(pod);
                let row_tint = if pod_is_unhealthy(&phase, &ready_str) {
                    Some(super::unhealthy_row_style(theme))
                } else {
                    None
                };
                let cell = |s: String| match row_tint {
                    Some(t) => Cell::from(s).style(t),
                    None => Cell::from(s),
                };
                let row = Row::new(vec![
                    Cell::from(marker),
                    cell(ns.to_string()),
                    cell(key.name.clone()),
                    cell(phase),
                    Cell::from(ready_str).style(super::merge_styles(row_tint, ready_style)),
                    cell(pod_restarts(pod).to_string()),
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
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" pods · {} ", self.snapshot.len())),
        );

        frame.render_widget(table, area);
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

    fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }

    fn selected_can_exec(&self) -> bool {
        let Some((_, pod)) = self.snapshot.get(self.selected) else {
            return false;
        };
        pod_phase(pod) == "Running"
            && pod
                .status
                .as_ref()
                .and_then(|s| s.container_statuses.as_ref())
                .map(|cs| cs.iter().any(|c| c.ready))
                .unwrap_or(false)
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

/// A pod is "unhealthy" (whole row painted with the muted red
/// tint) when its phase signals failure or it has 0 containers
/// ready out of N. Healthy Running pods, completed Succeeded pods,
/// and pending pods with no containers yet stay default-styled.
fn pod_is_unhealthy(phase: &str, ready: &str) -> bool {
    matches!(
        phase,
        "Failed" | "CrashLoopBackOff" | "Error" | "ImagePullBackOff" | "ErrImagePull"
    ) || super::ready_str_is_unhealthy(ready)
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

    #[test]
    fn render_selected_row_uses_themes_selection_fg() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (k, p) = make_pod_entry("default", "nginx");
        let mut view = PodsView::new();
        view.snapshot = vec![(k, p)];

        // Pick a theme whose selection_fg differs from terminal's
        // default cyan — solarized-light puts it on #268bd2 blue.
        let theme = crate::theme::Theme::embedded("solarized-light").unwrap();
        let want = theme.selection_fg.as_ratatui();

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();

        let buf = terminal.backend().buffer();
        let mut saw_marker_cell = false;
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                let cell = &buf[(x, y)];
                if cell.symbol() == "▎" {
                    saw_marker_cell = true;
                    assert_eq!(
                        cell.style().fg,
                        Some(want),
                        "selected-row marker should use theme selection_fg"
                    );
                }
            }
        }
        assert!(
            saw_marker_cell,
            "expected to find ▎ selection marker in rendered buffer"
        );
    }

    #[test]
    fn render_ready_cell_is_failed_red_when_zero_containers_ready() {
        use k8s_openapi::api::core::v1::{ContainerStatus, PodStatus};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // Pod with 3 containers, none ready -> READY column shows "0/3".
        let (k, mut p) = make_pod_entry("default", "broken");
        p.status = Some(PodStatus {
            container_statuses: Some(vec![
                ContainerStatus {
                    name: "a".into(),
                    ready: false,
                    ..Default::default()
                },
                ContainerStatus {
                    name: "b".into(),
                    ready: false,
                    ..Default::default()
                },
                ContainerStatus {
                    name: "c".into(),
                    ready: false,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        });
        let mut view = PodsView::new();
        view.snapshot = vec![(k, p)];

        let theme = crate::theme::Theme::terminal_default();
        let want_fg = theme.status.failed.as_ratatui();

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        // Find the leading '0' of "0/3" anywhere on the rendered grid.
        let mut found = false;
        for y in 0..buf.area().height {
            for x in 0..buf.area().width.saturating_sub(2) {
                if buf[(x, y)].symbol() == "0"
                    && buf[(x + 1, y)].symbol() == "/"
                    && buf[(x + 2, y)].symbol() == "3"
                {
                    let style = buf[(x, y)].style();
                    assert_eq!(
                        style.fg,
                        Some(want_fg),
                        "zero-ready READY cell should use status.failed fg"
                    );
                    assert!(
                        style.add_modifier.contains(ratatui::style::Modifier::BOLD),
                        "zero-ready READY cell should be bold"
                    );
                    found = true;
                }
            }
        }
        assert!(found, "expected '0/3' in rendered buffer");
    }

    #[test]
    fn render_ready_cell_is_unstyled_when_fully_ready() {
        use k8s_openapi::api::core::v1::{ContainerStatus, PodStatus};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (k, mut p) = make_pod_entry("default", "ok");
        p.status = Some(PodStatus {
            container_statuses: Some(vec![ContainerStatus {
                name: "a".into(),
                ready: true,
                ..Default::default()
            }]),
            ..Default::default()
        });
        let mut view = PodsView::new();
        view.snapshot = vec![(k, p)];

        let theme = crate::theme::Theme::terminal_default();
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        // Find "1/1" — its leading '1' cell should NOT carry the
        // failed fg.
        let want_fg = theme.status.failed.as_ratatui();
        for y in 0..buf.area().height {
            for x in 0..buf.area().width.saturating_sub(2) {
                if buf[(x, y)].symbol() == "1"
                    && buf[(x + 1, y)].symbol() == "/"
                    && buf[(x + 2, y)].symbol() == "1"
                {
                    let style = buf[(x, y)].style();
                    assert_ne!(
                        style.fg,
                        Some(want_fg),
                        "fully-ready cell should not carry status.failed fg"
                    );
                }
            }
        }
    }

    #[test]
    fn render_row_tinted_red_when_pod_is_zero_ready() {
        use k8s_openapi::api::core::v1::{ContainerStatus, PodStatus};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (k, mut p) = make_pod_entry("default", "broken");
        p.status = Some(PodStatus {
            phase: Some("Running".into()),
            container_statuses: Some(vec![ContainerStatus {
                name: "a".into(),
                ready: false,
                ..Default::default()
            }]),
            ..Default::default()
        });
        let mut view = PodsView::new();
        view.snapshot = vec![(k, p)];

        let theme = crate::theme::Theme::terminal_default();
        let want_fg = theme.status.failed.as_ratatui();

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        // The NAME cell ("broken") should carry the row tint.
        let mut found = false;
        for y in 0..buf.area().height {
            for x in 0..buf.area().width.saturating_sub(5) {
                let glyphs: String = (0..6).map(|i| buf[(x + i, y)].symbol()).collect();
                if glyphs == "broken" {
                    assert_eq!(buf[(x, y)].style().fg, Some(want_fg));
                    found = true;
                }
            }
        }
        assert!(found, "expected 'broken' name cell in buffer");
    }

    #[test]
    fn render_row_tinted_red_when_pod_phase_is_crashloopbackoff() {
        use k8s_openapi::api::core::v1::PodStatus;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (k, mut p) = make_pod_entry("default", "looping");
        p.status = Some(PodStatus {
            phase: Some("CrashLoopBackOff".into()),
            ..Default::default()
        });
        let mut view = PodsView::new();
        view.snapshot = vec![(k, p)];

        let theme = crate::theme::Theme::terminal_default();
        let want_fg = theme.status.failed.as_ratatui();

        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        let mut found = false;
        for y in 0..buf.area().height {
            for x in 0..buf.area().width.saturating_sub(6) {
                let glyphs: String = (0..7).map(|i| buf[(x + i, y)].symbol()).collect();
                if glyphs == "looping" {
                    assert_eq!(buf[(x, y)].style().fg, Some(want_fg));
                    found = true;
                }
            }
        }
        assert!(found, "expected 'looping' name cell in buffer");
    }

    #[test]
    fn render_row_not_tinted_when_pod_is_running_and_ready() {
        use k8s_openapi::api::core::v1::{ContainerStatus, PodStatus};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let (k, mut p) = make_pod_entry("default", "healthy");
        p.status = Some(PodStatus {
            phase: Some("Running".into()),
            container_statuses: Some(vec![ContainerStatus {
                name: "a".into(),
                ready: true,
                ..Default::default()
            }]),
            ..Default::default()
        });
        let mut view = PodsView::new();
        view.snapshot = vec![(k, p)];

        let theme = crate::theme::Theme::terminal_default();
        let want_fg = theme.status.failed.as_ratatui();

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f, f.area(), &theme)).unwrap();
        let buf = terminal.backend().buffer();

        for y in 0..buf.area().height {
            for x in 0..buf.area().width.saturating_sub(6) {
                let glyphs: String = (0..7).map(|i| buf[(x + i, y)].symbol()).collect();
                if glyphs == "healthy" {
                    assert_ne!(buf[(x, y)].style().fg, Some(want_fg));
                }
            }
        }
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
