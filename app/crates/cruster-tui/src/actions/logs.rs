//! Logs action: streams a pod's container logs into a scrollable pane.
//! Supports grep filtering and follow-mode toggle.
//!
//! The streaming task owns one half of an `Arc<Mutex<Vec<String>>>`;
//! the render path locks it briefly each frame to copy out a snapshot.
//! That avoids the async-in-sync-closure problem with `terminal.draw`.

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use futures::{AsyncBufReadExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use tokio::task::JoinHandle;

/// Maximum lines kept in memory per stream. Older lines are dropped.
const MAX_LINES: usize = 5000;

#[derive(Default)]
pub struct LogsPane {
    open: bool,
    title: String,
    lines: Arc<Mutex<Vec<String>>>,
    grep: String,
    grep_active: bool,
    scroll: u16,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for LogsPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogsPane")
            .field("open", &self.open)
            .field("title", &self.title)
            .field("grep", &self.grep)
            .field("grep_active", &self.grep_active)
            .field("scroll", &self.scroll)
            .finish()
    }
}

impl LogsPane {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Start streaming logs for the given pod. Closes any existing
    /// stream. Returns immediately; the stream runs on a tokio task.
    pub fn open(&mut self, client: Client, key: &ResourceKey) {
        self.close();
        let Some(ns) = key.namespace.clone() else {
            return;
        };
        let name = key.name.clone();
        self.title = format!("Pod/{ns}/{name}");
        let lines = self.lines.clone();
        let handle = tokio::spawn(async move {
            let api: Api<Pod> = Api::namespaced(client, &ns);
            let params = LogParams {
                follow: true,
                tail_lines: Some(500),
                ..Default::default()
            };
            match api.log_stream(&name, &params).await {
                Ok(stream) => {
                    let mut reader = stream.lines();
                    loop {
                        match reader.try_next().await {
                            Ok(Some(line)) => {
                                let mut buf = lines.lock().unwrap();
                                buf.push(line);
                                if buf.len() > MAX_LINES {
                                    let drop = buf.len() - MAX_LINES;
                                    buf.drain(0..drop);
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                lines.lock().unwrap().push(format!("[stream error: {e}]"));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    lines
                        .lock()
                        .unwrap()
                        .push(format!("[error opening log stream: {e}]"));
                }
            }
        });
        self.handle = Some(handle);
        self.open = true;
        self.scroll = 0;
        self.grep.clear();
        self.grep_active = false;
    }

    pub fn close(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
        if let Ok(mut buf) = self.lines.lock() {
            buf.clear();
        }
        self.open = false;
        self.scroll = 0;
        self.grep.clear();
        self.grep_active = false;
    }

    /// Returns `true` if the key was consumed by the pane.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if !self.open || key.kind != KeyEventKind::Press {
            return false;
        }
        if self.grep_active {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.grep_active = false;
                    true
                }
                KeyCode::Backspace => {
                    self.grep.pop();
                    true
                }
                KeyCode::Char(c) => {
                    self.grep.push(c);
                    true
                }
                _ => true,
            }
        } else {
            match key.code {
                KeyCode::Esc => {
                    self.close();
                    true
                }
                KeyCode::Char('/') => {
                    self.grep_active = true;
                    self.grep.clear();
                    true
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.scroll = self.scroll.saturating_add(1);
                    true
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.scroll = self.scroll.saturating_sub(1);
                    true
                }
                KeyCode::PageDown => {
                    self.scroll = self.scroll.saturating_add(20);
                    true
                }
                KeyCode::PageUp => {
                    self.scroll = self.scroll.saturating_sub(20);
                    true
                }
                _ => true,
            }
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if !self.open {
            return;
        }
        let lines = self.lines.lock().unwrap();
        let filtered: Vec<&String> = if self.grep.is_empty() {
            lines.iter().collect()
        } else {
            lines.iter().filter(|l| l.contains(&self.grep)).collect()
        };
        let body = filtered
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let title = if self.grep.is_empty() {
            format!(" logs: {} — / grep · esc closes ", self.title)
        } else {
            let suffix = if self.grep_active { " (typing)" } else { "" };
            format!(
                " logs: {} — grep:'{}' ({}/{}){} ",
                self.title,
                self.grep,
                filtered.len(),
                lines.len(),
                suffix
            )
        };
        let para = Paragraph::new(body)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0));
        frame.render_widget(para, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn closed_pane_swallows_no_keys() {
        let mut p = LogsPane::new();
        assert!(!p.handle_key(press(KeyCode::Char('j'))));
    }

    #[test]
    fn slash_enters_grep_mode() {
        let mut p = LogsPane::new();
        p.open = true;
        assert!(p.handle_key(press(KeyCode::Char('/'))));
        assert!(p.grep_active);
    }

    #[test]
    fn typing_in_grep_mode_buffers_chars() {
        let mut p = LogsPane::new();
        p.open = true;
        p.grep_active = true;
        p.handle_key(press(KeyCode::Char('e')));
        p.handle_key(press(KeyCode::Char('r')));
        p.handle_key(press(KeyCode::Char('r')));
        assert_eq!(p.grep, "err");
    }

    #[test]
    fn enter_exits_grep_typing_mode_but_keeps_filter() {
        let mut p = LogsPane::new();
        p.open = true;
        p.grep_active = true;
        p.grep = "err".into();
        p.handle_key(press(KeyCode::Enter));
        assert!(!p.grep_active);
        assert_eq!(p.grep, "err");
    }

    #[test]
    fn esc_in_grep_mode_exits_grep_typing_but_keeps_pane_open() {
        let mut p = LogsPane::new();
        p.open = true;
        p.grep_active = true;
        p.handle_key(press(KeyCode::Esc));
        assert!(!p.grep_active);
        assert!(p.open);
    }

    #[test]
    fn scroll_advances_and_clamps_at_zero() {
        let mut p = LogsPane::new();
        p.open = true;
        p.handle_key(press(KeyCode::Char('j')));
        p.handle_key(press(KeyCode::Char('j')));
        assert_eq!(p.scroll, 2);
        p.handle_key(press(KeyCode::Char('k')));
        p.handle_key(press(KeyCode::Char('k')));
        p.handle_key(press(KeyCode::Char('k')));
        assert_eq!(p.scroll, 0);
    }
}
