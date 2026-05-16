//! Secrets table view.
//!
//! **Hard invariant:** never render any secret data value, ever. The
//! view shows only the key count. Use describe or YAML view to
//! inspect contents (which will redact per the design principle in
//! the spec).

use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use k8s_openapi::api::core::v1::Secret;
use ratatui::layout::Constraint;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::app::LoopState;
use crate::overlays::search::Filter;
use crate::view::ResourceView;
use crate::views::configmaps::metadata_age;

#[derive(Debug, Default)]
pub struct SecretsView {
    selected: usize,
    snapshot: Vec<(ResourceKey, Secret)>,
    filter: Filter,
}

impl SecretsView {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ResourceView for SecretsView {
    fn id(&self) -> &'static str {
        "secrets"
    }

    async fn refresh(&mut self, registry: &StoreRegistry) {
        let snap = registry.secrets.snapshot().await;
        self.snapshot = crate::overlays::search::apply(&self.filter, snap, |s| s.type_.clone());
        if self.selected > 0 && self.selected >= self.snapshot.len() {
            self.selected = self.snapshot.len().saturating_sub(1);
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let header = Row::new(vec!["NAMESPACE", "NAME", "TYPE", "DATA", "AGE"])
            .style(Style::default().add_modifier(Modifier::BOLD));

        let table_rows: Vec<Row> = self
            .snapshot
            .iter()
            .map(|(key, s)| {
                let ns = key.namespace.as_deref().unwrap_or("-");
                Row::new(vec![
                    Cell::from(ns.to_string()),
                    Cell::from(key.name.clone()),
                    Cell::from(secret_type(s)),
                    Cell::from(secret_data_count(s).to_string()),
                    Cell::from(metadata_age(&s.metadata)),
                ])
            })
            .collect();

        let widths = [
            Constraint::Length(20),
            Constraint::Min(20),
            Constraint::Length(28),
            Constraint::Length(8),
            Constraint::Length(10),
        ];

        let table = Table::new(table_rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(
                " secrets ({}) — j/k move · :kind switch · q quit ",
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
        // Redact data and stringData before serialising so the YAML
        // dump never reveals secret values, even via describe/y.
        let mut redacted = obj.clone();
        if let Some(d) = &mut redacted.data {
            for (_, v) in d.iter_mut() {
                *v = k8s_openapi::ByteString(b"<redacted>".to_vec());
            }
        }
        if let Some(d) = &mut redacted.string_data {
            for (_, v) in d.iter_mut() {
                *v = "<redacted>".into();
            }
        }
        let yaml = serde_yaml::to_string(&redacted).ok()?;
        Some((key.to_string(), yaml))
    }

    fn selected_key(&self) -> Option<ResourceKey> {
        self.snapshot.get(self.selected).map(|(k, _)| k.clone())
    }

    fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }
}

fn secret_type(s: &Secret) -> String {
    s.type_.clone().unwrap_or_else(|| "Opaque".into())
}

fn secret_data_count(s: &Secret) -> usize {
    s.data.as_ref().map(|d| d.len()).unwrap_or(0)
        + s.string_data.as_ref().map(|d| d.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::ByteString;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::collections::BTreeMap;

    fn make_secret_with_data(name: &str, data: Vec<(&str, &str)>) -> Secret {
        let mut map: BTreeMap<String, ByteString> = BTreeMap::new();
        for (k, v) in data {
            map.insert(k.into(), ByteString(v.as_bytes().to_vec()));
        }
        Secret {
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some("default".into()),
                ..Default::default()
            },
            type_: Some("Opaque".into()),
            data: Some(map),
            ..Default::default()
        }
    }

    #[test]
    fn secret_data_count_combines_data_and_string_data() {
        let mut data: BTreeMap<String, ByteString> = BTreeMap::new();
        data.insert("a".into(), ByteString(b"x".to_vec()));
        data.insert("b".into(), ByteString(b"y".to_vec()));
        let mut string_data: BTreeMap<String, String> = BTreeMap::new();
        string_data.insert("c".into(), "z".into());
        let s = Secret {
            data: Some(data),
            string_data: Some(string_data),
            ..Default::default()
        };
        assert_eq!(secret_data_count(&s), 3);
    }

    #[test]
    fn secret_type_defaults_to_opaque() {
        let s = Secret::default();
        assert_eq!(secret_type(&s), "Opaque");
    }

    /// Critical invariant test: the rendered TUI output must never
    /// contain any secret value, even base64-encoded or otherwise
    /// transformed. This test renders the view and verifies the
    /// secret values do not appear in the buffer string.
    #[test]
    fn render_never_includes_secret_values() {
        let secret = make_secret_with_data(
            "db-creds",
            vec![
                ("password", "SUPER_SECRET_PWD_12345"),
                ("api-token", "tok_abcdefghijklmnop"),
            ],
        );

        let key = ResourceKey::namespaced("Secret", "default", "db-creds");
        let mut view = SecretsView::new();
        view.snapshot = vec![(key, secret)];

        let backend = TestBackend::new(120, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| view.render(f)).unwrap();
        let rendered = buffer_as_string(terminal.backend().buffer());

        assert!(
            !rendered.contains("SUPER_SECRET_PWD_12345"),
            "secret value leaked to TUI render"
        );
        assert!(
            !rendered.contains("tok_abcdefghijklmnop"),
            "secret value leaked to TUI render"
        );
        // The base64 of "SUPER_SECRET_PWD_12345" should also not appear.
        // (k8s data is base64 on the wire, but k8s-openapi already decodes
        // it into ByteString. Belt-and-suspenders: check the decoded form
        // doesn't appear.)
        assert!(!rendered.contains("U1VQRVJfU0VDUkVUX1BXRF8xMjM0NQ=="));
    }

    fn buffer_as_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area().height {
            for x in 0..buf.area().width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }
}
