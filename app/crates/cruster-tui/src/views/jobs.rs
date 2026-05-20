//! Jobs table view.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::batch::v1::Job;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;

#[derive(Debug, Default)]
pub struct JobsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Job)>,
    filter: Filter,
}

impl JobsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for JobsView {
    fn id(&self) -> &'static str {
        "jobs"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.jobs.snapshot().await;
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
            "COMPLETIONS",
            "DURATION",
            "AGE",
        ])
        .style(Style::default().fg(theme.header_fg.as_ratatui()));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, job))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(completions(job)),
                    Cell::from(duration(job)),
                    Cell::from(metadata_age(&job.metadata)),
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
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(8),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" jobs · {} ", self.snapshot.len())),
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

fn completions(job: &Job) -> String {
    let desired = job.spec.as_ref().and_then(|s| s.completions).unwrap_or(1);
    let succeeded = job.status.as_ref().and_then(|s| s.succeeded).unwrap_or(0);
    format!("{succeeded}/{desired}")
}

fn duration(job: &Job) -> String {
    let Some(status) = &job.status else {
        return "-".into();
    };
    let start = status.start_time.as_ref().map(|t| t.0);
    let end = status.completion_time.as_ref().map(|t| t.0);
    match (start, end) {
        (Some(s), Some(e)) => {
            let secs = (e - s).num_seconds().max(0);
            format_duration(secs)
        }
        (Some(s), None) => {
            let secs = (chrono::Utc::now() - s).num_seconds().max(0);
            format_duration(secs)
        }
        _ => "-".into(),
    }
}

fn format_duration(secs: i64) -> String {
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
    use k8s_openapi::api::batch::v1::JobStatus;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn make_job(ns: &str, name: &str, succeeded: i32, completions: i32) -> Job {
        Job {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                ..Default::default()
            },
            spec: Some(k8s_openapi::api::batch::v1::JobSpec {
                completions: Some(completions),
                ..Default::default()
            }),
            status: Some(JobStatus {
                succeeded: Some(succeeded),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn renders_columns_for_three_resources_no_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = JobsView::new();
        view.snapshot = vec![
            (
                ResourceKey::namespaced("Job", "default", "backup"),
                make_job("default", "backup", 1, 1),
            ),
            (
                ResourceKey::namespaced("Job", "default", "migration"),
                make_job("default", "migration", 0, 1),
            ),
            (
                ResourceKey::namespaced("Job", "batch", "report"),
                make_job("batch", "report", 3, 5),
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

        let mut view = JobsView::new();
        view.snapshot = vec![(
            ResourceKey::namespaced("Job", "default", "my-job"),
            make_job("default", "my-job", 1, 1),
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
    async fn refresh_loads_jobs_from_registry() {
        let r = StoreRegistry::new();
        r.jobs
            .upsert(
                ResourceKey::namespaced("Job", "default", "my-job"),
                make_job("default", "my-job", 1, 1),
            )
            .await;
        let mut v = JobsView::new();
        v.refresh(&r).await;
        assert_eq!(v.snapshot.len(), 1);
    }
}
