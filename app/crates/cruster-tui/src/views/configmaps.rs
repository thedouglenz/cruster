//! ConfigMaps table view.
//!
//! Renders only the data-key count, never the data values themselves.
//! Use the describe / YAML view for actual contents.

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::theme::Theme;
use crate::view::ResourceView;
use crate::views::events::human_age;

#[derive(Debug, Default)]
pub struct ConfigMapsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, ConfigMap)>,
    filter: Filter,
}

impl ConfigMapsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for ConfigMapsView {
    fn id(&self) -> &'static str {
        "configmaps"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.configmaps.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |_| None);
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let header = Row::new(vec!["", "NAMESPACE", "NAME", "DATA", "AGE"])
            .style(Style::default().fg(theme.header_fg.as_ratatui()));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .enumerate()
            .map(|(i, (key, cm))| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                let marker = if i == self.selected { "▎" } else { " " };
                let row = Row::new(vec![
                    Cell::from(marker),
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(cm_data_count(cm).to_string()),
                    Cell::from(metadata_age(&cm.metadata)),
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
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths).header(header).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" configmaps · {} ", self.snapshot.len())),
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

fn cm_data_count(cm: &ConfigMap) -> usize {
    cm.data.as_ref().map(|d| d.len()).unwrap_or(0)
        + cm.binary_data.as_ref().map(|d| d.len()).unwrap_or(0)
}

pub fn metadata_age(m: &ObjectMeta) -> String {
    m.creation_timestamp
        .as_ref()
        .map(|t| human_age(t.0))
        .unwrap_or_else(|| "?".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn cm_data_count_zero_when_empty() {
        let cm = ConfigMap::default();
        assert_eq!(cm_data_count(&cm), 0);
    }

    #[test]
    fn render_selected_row_uses_themes_selection_fg() {
        use cruster_core::ResourceKey;
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut view = ConfigMapsView::new();
        view.snapshot = vec![(
            ResourceKey::namespaced("ConfigMap", "default", "cm"),
            ConfigMap {
                metadata: ObjectMeta {
                    name: Some("cm".into()),
                    namespace: Some("default".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
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

    #[test]
    fn cm_data_count_includes_both_data_and_binary_data() {
        let mut data = BTreeMap::new();
        data.insert("a".into(), "x".into());
        data.insert("b".into(), "y".into());
        let mut binary_data = BTreeMap::new();
        binary_data.insert("c".into(), k8s_openapi::ByteString(vec![1, 2, 3]));
        let cm = ConfigMap {
            data: Some(data),
            binary_data: Some(binary_data),
            ..Default::default()
        };
        assert_eq!(cm_data_count(&cm), 3);
    }
}
