//! Vim-style command mode for switching views.
//!
//! Press `:`, type a kind alias, hit Enter to switch.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

/// State of the command-mode line.
#[derive(Debug, Default)]
pub struct CommandLine {
    /// The text the user has typed so far (without the leading `:`).
    buffer: String,
    /// `true` when the user has pressed `:` and is typing.
    active: bool,
}

/// What the App should do after a command-mode key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    /// Stay in current mode; nothing to do.
    None,
    /// Switch to the view identified by the given id ("pods", "deployments", …).
    SwitchTo(String),
    /// Cancel command mode (clear buffer, deactivate).
    Cancel,
    /// User entered an unknown alias; show an error toast.
    UnknownAlias(String),
}

impl CommandLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Activate command mode (called by the App when `:` is pressed
    /// outside command mode).
    pub fn activate(&mut self) {
        self.active = true;
        self.buffer.clear();
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.buffer.clear();
    }

    /// Handle a key event while in command mode. Returns the action
    /// the App should take.
    pub fn handle_key(&mut self, key: KeyEvent) -> CommandAction {
        if key.kind != KeyEventKind::Press {
            return CommandAction::None;
        }
        match key.code {
            KeyCode::Esc => {
                self.deactivate();
                CommandAction::Cancel
            }
            KeyCode::Enter => {
                let buf = std::mem::take(&mut self.buffer);
                self.active = false;
                match resolve_alias(&buf) {
                    Some(id) => CommandAction::SwitchTo(id.to_string()),
                    None => CommandAction::UnknownAlias(buf),
                }
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                CommandAction::None
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                CommandAction::None
            }
            _ => CommandAction::None,
        }
    }
}

/// Resolve a user-typed alias to a view id. Returns `None` for unknown
/// aliases.
pub fn resolve_alias(alias: &str) -> Option<&'static str> {
    match alias {
        "dash" | "dashboard" | "pulse" => Some("dashboard"),
        "po" | "pod" | "pods" => Some("pods"),
        "deploy" | "deployment" | "deployments" => Some("deployments"),
        "svc" | "service" | "services" => Some("services"),
        "no" | "node" | "nodes" => Some("nodes"),
        "ev" | "event" | "events" => Some("events"),
        "cm" | "configmap" | "configmaps" => Some("configmaps"),
        "sec" | "secret" | "secrets" => Some("secrets"),
        "ns" | "namespace" | "namespaces" => Some("namespaces"),
        _ => None,
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
    fn typing_buffers_chars() {
        let mut c = CommandLine::new();
        c.activate();
        c.handle_key(press(KeyCode::Char('d')));
        c.handle_key(press(KeyCode::Char('e')));
        c.handle_key(press(KeyCode::Char('p')));
        assert_eq!(c.buffer(), "dep");
    }

    #[test]
    fn enter_with_known_alias_returns_switch() {
        let mut c = CommandLine::new();
        c.activate();
        for ch in "deploy".chars() {
            c.handle_key(press(KeyCode::Char(ch)));
        }
        let action = c.handle_key(press(KeyCode::Enter));
        assert_eq!(action, CommandAction::SwitchTo("deployments".into()));
        assert!(!c.is_active());
    }

    #[test]
    fn enter_with_unknown_alias_returns_error() {
        let mut c = CommandLine::new();
        c.activate();
        for ch in "wat".chars() {
            c.handle_key(press(KeyCode::Char(ch)));
        }
        let action = c.handle_key(press(KeyCode::Enter));
        assert_eq!(action, CommandAction::UnknownAlias("wat".into()));
    }

    #[test]
    fn esc_cancels() {
        let mut c = CommandLine::new();
        c.activate();
        c.handle_key(press(KeyCode::Char('x')));
        let action = c.handle_key(press(KeyCode::Esc));
        assert_eq!(action, CommandAction::Cancel);
        assert!(!c.is_active());
        assert_eq!(c.buffer(), "");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut c = CommandLine::new();
        c.activate();
        for ch in "abc".chars() {
            c.handle_key(press(KeyCode::Char(ch)));
        }
        c.handle_key(press(KeyCode::Backspace));
        assert_eq!(c.buffer(), "ab");
    }

    #[test]
    fn resolve_alias_recognizes_canonical_and_short() {
        assert_eq!(resolve_alias("pods"), Some("pods"));
        assert_eq!(resolve_alias("po"), Some("pods"));
        assert_eq!(resolve_alias("svc"), Some("services"));
        assert_eq!(resolve_alias("nope"), None);
    }

    #[test]
    fn resolve_alias_recognises_dashboard_aliases() {
        assert_eq!(resolve_alias("dash"), Some("dashboard"));
        assert_eq!(resolve_alias("dashboard"), Some("dashboard"));
        assert_eq!(resolve_alias("pulse"), Some("dashboard"));
    }
}
