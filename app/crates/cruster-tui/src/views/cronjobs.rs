//! CronJobs table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::batch::v1::CronJob;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct CronJobsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, CronJob)>,
    filter: Filter,
}

impl CronJobsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for CronJobsView {
    fn id(&self) -> &'static str {
        "cronjobs"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.cronjobs.snapshot().await;
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
            "SCHEDULE",
            "SUSPEND",
            "ACTIVE",
            "LAST SCHEDULE",
            "AGE",
        ])
        .style(Style::default().fg(theme.header_fg.as_ratatui()));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, cj))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(schedule(cj)),
                    Cell::from(suspend(cj)),
                    Cell::from(active(cj)),
                    Cell::from(last_schedule(cj)),
                    Cell::from(metadata_age(&cj.metadata)),
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
            Constraint::Length(20),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(15),
            Constraint::Length(8),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" cronjobs · {} ", self.snapshot.len())),
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

fn schedule(cj: &CronJob) -> String {
    cj.spec
        .as_ref()
        .map(|s| s.schedule.clone())
        .unwrap_or_else(|| "-".into())
}

fn suspend(cj: &CronJob) -> String {
    cj.spec
        .as_ref()
        .and_then(|s| s.suspend)
        .map(|s| if s { "True" } else { "False" })
        .unwrap_or("False")
        .into()
}

fn active(cj: &CronJob) -> String {
    cj.status
        .as_ref()
        .and_then(|s| s.active.as_ref())
        .map(|a| a.len().to_string())
        .unwrap_or_else(|| "0".into())
}

fn last_schedule(cj: &CronJob) -> String {
    let Some(ts) = cj
        .status
        .as_ref()
        .and_then(|s| s.last_schedule_time.as_ref())
    else {
        return "-".into();
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
    use k8s_openapi::api::batch::v1::{CronJobSpec, CronJobStatus, JobTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_cronjob(ns: &str, name: &str, schedule: &str, suspend: bool) -> CronJob {
        CronJob {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: Some(CronJobSpec {
                schedule: schedule.into(),
                suspend: Some(suspend),
                job_template: JobTemplateSpec::default(),
                ..Default::default()
            }),
            status: Some(CronJobStatus {
                active: Some(vec![]),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn renders_columns_for_three_resources_no_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = CronJobsView::new();
        view.snapshot = vec![
            (
                ResourceKey::namespaced("CronJob", "default", "backup"),
                make_cronjob("default", "backup", "0 2 * * *", false),
            ),
            (
                ResourceKey::namespaced("CronJob", "default", "cleanup"),
                make_cronjob("default", "cleanup", "0 */6 * * *", true),
            ),
            (
                ResourceKey::namespaced("CronJob", "prod", "report"),
                make_cronjob("prod", "report", "0 9 * * 1", false),
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

        let mut view = CronJobsView::new();
        view.snapshot = vec![(
            ResourceKey::namespaced("CronJob", "default", "backup"),
            make_cronjob("default", "backup", "0 2 * * *", false),
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
    async fn refresh_loads_cronjobs_from_registry() {
        let r = StoreRegistry::new();
        r.cronjobs
            .upsert(
                ResourceKey::namespaced("CronJob", "default", "backup"),
                make_cronjob("default", "backup", "0 2 * * *", false),
            )
            .await;
        let mut v = CronJobsView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
