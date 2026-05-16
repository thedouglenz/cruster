//! Pods table view.

use cruster_core::ResourceKey;
use k8s_openapi::api::core::v1::Pod;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

#[derive(Debug, Default)]
pub struct PodsView {
    selected: usize,
}

impl PodsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self, row_count: usize) {
        if row_count == 0 {
            self.selected = 0;
            return;
        }
        let max = row_count.saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    pub fn move_to_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_to_bottom(&mut self, row_count: usize) {
        self.selected = row_count.saturating_sub(1);
    }

    pub fn render(&self, frame: &mut Frame<'_>, rows: &[(ResourceKey, Pod)]) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "STATUS", "READY", "RESTARTS"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = rows
            .iter()
            .map(|(key, pod)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let status = pod_phase(pod);
                let ready = pod_ready(pod);
                let restarts = pod_restarts(pod);
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(status),
                    Cell::from(ready),
                    Cell::from(restarts.to_string()),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" pods ({}) — j/k move · q quit ", rows.len())),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if !rows.is_empty() {
            state.select(Some(self.selected.min(rows.len() - 1)));
        }

        frame.render_stateful_widget(table, area, &mut state);
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

    #[test]
    fn move_down_on_empty_stays_at_zero() {
        let mut v = PodsView::new();
        v.move_down(0);
        assert_eq!(v.selected(), 0);
    }

    #[test]
    fn move_down_clamps_to_last_row() {
        let mut v = PodsView::new();
        for _ in 0..10 {
            v.move_down(3);
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
        v.move_to_bottom(0);
        assert_eq!(v.selected(), 0);
    }
}
