//! Faceted search filter.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;

use crate::overlay::{Overlay, OverlayResult};

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub raw: String,
    pub tokens: Vec<FilterToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterToken {
    Namespace(String),
    Status(String),
    NameContains(String),
    NameFuzzy(String),
}

impl Filter {
    pub fn parse(raw: &str) -> Self {
        let mut tokens = Vec::new();
        for part in raw.split_whitespace() {
            if let Some(rest) = part.strip_prefix('~') {
                tokens.push(FilterToken::NameContains(rest.into()));
            } else if let Some((k, v)) = part.split_once(':') {
                match k {
                    "ns" | "namespace" => tokens.push(FilterToken::Namespace(v.into())),
                    "status" => tokens.push(FilterToken::Status(v.into())),
                    _ => tokens.push(FilterToken::NameFuzzy(part.into())),
                }
            } else {
                tokens.push(FilterToken::NameFuzzy(part.into()));
            }
        }
        Self {
            raw: raw.into(),
            tokens,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Test whether a `(namespace, name, status)` tuple passes.
    pub fn matches(&self, namespace: Option<&str>, name: &str, status: Option<&str>) -> bool {
        for tok in &self.tokens {
            match tok {
                FilterToken::Namespace(ns) => {
                    if !namespace.map(|n| n.starts_with(ns.as_str())).unwrap_or(false) {
                        return false;
                    }
                }
                FilterToken::Status(s) => {
                    if status.map(|st| st != s).unwrap_or(true) {
                        return false;
                    }
                }
                FilterToken::NameContains(sub) => {
                    if !name.contains(sub.as_str()) {
                        return false;
                    }
                }
                FilterToken::NameFuzzy(q) => {
                    let lq = q.to_lowercase();
                    if !name.to_lowercase().contains(&lq) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[derive(Default)]
pub struct SearchPrompt {
    buffer: String,
}

impl SearchPrompt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn current_filter(&self) -> Filter {
        Filter::parse(&self.buffer)
    }
}

/// Apply a filter to a vector of `(ResourceKey, T)` pairs. `status_fn`
/// extracts an optional status string from each item; kinds that don't
/// have a status concept should pass `|_| None`.
pub fn apply<T, F>(
    filter: &Filter,
    items: Vec<(cruster_core::ResourceKey, T)>,
    status_fn: F,
) -> Vec<(cruster_core::ResourceKey, T)>
where
    F: Fn(&T) -> Option<String>,
{
    if filter.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|(k, obj)| {
            filter.matches(k.namespace.as_deref(), &k.name, status_fn(obj).as_deref())
        })
        .collect()
}

impl Overlay for SearchPrompt {
    fn live_filter_buffer(&self) -> Option<&str> {
        Some(&self.buffer)
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => OverlayResult::Close,
            KeyCode::Backspace => {
                self.buffer.pop();
                OverlayResult::KeepOpen
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let line = format!("/ {}_", self.buffer);
        let bar = Paragraph::new(line).style(Style::default().bg(Color::Blue));
        let rect = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(3),
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ns_token() {
        let f = Filter::parse("ns:prod");
        assert_eq!(f.tokens, vec![FilterToken::Namespace("prod".into())]);
    }

    #[test]
    fn parses_multiple_tokens() {
        let f = Filter::parse("ns:kube-system status:Running");
        assert_eq!(
            f.tokens,
            vec![
                FilterToken::Namespace("kube-system".into()),
                FilterToken::Status("Running".into()),
            ]
        );
    }

    #[test]
    fn fuzzy_fallback_for_bare_words() {
        let f = Filter::parse("nginx");
        assert_eq!(f.tokens, vec![FilterToken::NameFuzzy("nginx".into())]);
    }

    #[test]
    fn empty_filter_matches_everything() {
        let f = Filter::parse("");
        assert!(f.is_empty());
        assert!(f.matches(Some("any"), "any", Some("any")));
    }

    #[test]
    fn ns_filter_requires_prefix_match() {
        let f = Filter::parse("ns:kube");
        assert!(f.matches(Some("kube-system"), "foo", Some("Running")));
        assert!(!f.matches(Some("default"), "foo", Some("Running")));
    }

    #[test]
    fn status_filter_requires_exact_match() {
        let f = Filter::parse("status:Running");
        assert!(f.matches(Some("default"), "foo", Some("Running")));
        assert!(!f.matches(Some("default"), "foo", Some("Failed")));
    }

    #[test]
    fn name_contains_with_tilde() {
        let f = Filter::parse("~web");
        assert!(f.matches(None, "webserver", None));
        assert!(!f.matches(None, "database", None));
    }
}
