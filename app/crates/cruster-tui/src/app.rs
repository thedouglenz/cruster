//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use cruster_core::Environment;
use cruster_kube::StoreRegistry;
use kube::Client;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use ratatui::Terminal;

use crate::actions::describe::DescribePane;
use crate::actions::logs::LogsPane;
use crate::actions::port_forward::{PortForward, PortForwards};
use crate::command::{CommandAction, CommandLine};
use crate::overlay::{Overlay, OverlayResult};
#[allow(unused_imports)]
use crate::overlays::palette::{EntryKind, Palette, PaletteEntry};
use crate::overlays::search::{Filter, SearchPrompt};
use crate::view::ResourceView;
use crate::views::configmaps::ConfigMapsView;
use crate::views::deployments::DeploymentsView;
use crate::views::events::EventsView;
use crate::views::namespaces::NamespacesView;
use crate::views::nodes::NodesView;
use crate::views::pods::PodsView;
use crate::views::secrets::SecretsView;
use crate::views::services::ServicesView;
use cruster_core::ResourceKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

/// Active prompt for a port-forward mapping ("local:remote").
struct PortForwardPrompt {
    pod_key: ResourceKey,
    buffer: String,
}

pub struct App {
    registry: StoreRegistry,
    client: Option<Client>,
    current_view: Box<dyn ResourceView>,
    actions: crate::action::ActionRegistry,
    command: CommandLine,
    describe_pane: DescribePane,
    logs_pane: LogsPane,
    port_forwards: PortForwards,
    port_forward_prompt: Option<PortForwardPrompt>,
    overlay: Option<Box<dyn Overlay>>,
    history: crate::history::History,
    loaded_workflows: Vec<crate::workflows::Workflow>,
    context: String,
    environment: Environment,
    read_only: bool,
    pending_open_relationships: bool,
    toast: Option<String>,
}

impl App {
    pub fn new(registry: StoreRegistry, client: Option<Client>) -> Self {
        let context = load_current_context();
        let environment = crate::safety::SafetyConfig::load_or_default().classify(&context);
        let read_only = environment.requires_confirmation();
        Self {
            registry,
            client,
            current_view: Box::new(PodsView::new()),
            actions: crate::action_shipped::default_registry(),
            command: CommandLine::new(),
            describe_pane: DescribePane::new(),
            logs_pane: LogsPane::new(),
            port_forwards: PortForwards::new(),
            port_forward_prompt: None,
            overlay: None,
            history: {
                let mut h = crate::history::History::with_capacity(200);
                h.record("pods", None);
                h
            },
            loaded_workflows: Vec::new(),
            context,
            environment,
            read_only,
            pending_open_relationships: false,
            toast: None,
        }
    }

    fn open_workflows_palette(&mut self) {
        let workflows = crate::workflows::load_all();
        if workflows.is_empty() {
            self.toast = Some("no workflows in ~/.config/cruster/workflows/ — see the spec".into());
            return;
        }
        let entries: Vec<PaletteEntry> = workflows
            .iter()
            .enumerate()
            .map(|(i, w)| PaletteEntry {
                id: format!("wf:{i}"),
                label: format!("Workflow: {}", w.name),
                kind: EntryKind::View,
            })
            .collect();
        self.loaded_workflows = workflows;
        self.overlay = Some(Box::new(Palette::new(entries)));
    }

    fn run_workflow(&mut self, index: usize) {
        let Some(workflow) = self.loaded_workflows.get(index).cloned() else {
            self.toast = Some(format!("workflow index out of range: {index}"));
            return;
        };
        for step in &workflow.steps {
            match step {
                crate::workflows::Step::SwitchView(id) => match Self::view_for_id(id) {
                    Some(v) => {
                        self.current_view = v;
                        self.history.record(id, None);
                    }
                    None => self.toast = Some(format!("workflow: unknown view {id}")),
                },
                crate::workflows::Step::SetFilter(query) => {
                    self.current_view.set_filter(Filter::parse(query));
                }
            }
        }
        self.toast = Some(format!("ran workflow: {}", workflow.name));
    }

    fn open_history_palette(&mut self) {
        let entries: Vec<PaletteEntry> = self
            .history
            .ranked()
            .into_iter()
            .map(|item| PaletteEntry {
                id: item.view_id.clone(),
                label: match &item.key {
                    Some(k) => format!("{} ({} visits)", k, item.visit_count),
                    None => format!("View: {} ({} visits)", item.view_id, item.visit_count),
                },
                kind: EntryKind::View,
            })
            .collect();
        if entries.is_empty() {
            self.toast = Some("no history yet".into());
            return;
        }
        self.overlay = Some(Box::new(Palette::new(entries)));
    }

    fn open_palette(&mut self) {
        let mut entries: Vec<PaletteEntry> = self
            .actions
            .all()
            .iter()
            .map(|a| PaletteEntry {
                id: a.id().to_string(),
                label: a.label().to_string(),
                kind: EntryKind::Action,
            })
            .collect();
        for view_id in [
            "pods",
            "deployments",
            "services",
            "nodes",
            "events",
            "configmaps",
            "secrets",
            "namespaces",
        ] {
            entries.push(PaletteEntry {
                id: view_id.to_string(),
                label: format!("View: {}", view_id),
                kind: EntryKind::View,
            });
        }
        self.overlay = Some(Box::new(Palette::new(entries)));
    }

    /// Dispatch an action by id. Mirrors the per-key handlers; invoked
    /// either via the keyboard shortcut or via the command palette.
    fn invoke_action(&mut self, id: &str) -> LoopState {
        match id {
            "describe" => {
                if let Some((title, yaml)) = self.current_view.selected_yaml() {
                    self.describe_pane.open(title, yaml);
                } else {
                    self.toast = Some("nothing selected".into());
                }
                LoopState::Continue
            }
            "logs" => {
                self.open_logs_for_selection();
                LoopState::Continue
            }
            "exec" => {
                self.exec_into_selection();
                LoopState::Continue
            }
            "port-forward" => {
                self.start_port_forward_prompt();
                LoopState::Continue
            }
            "edit" => {
                self.edit_selection_yaml();
                LoopState::Continue
            }
            "switch-kind" => {
                self.command.activate();
                LoopState::Continue
            }
            "quit" => LoopState::Quit,
            "copy-kubectl" => {
                self.copy_kubectl_for_selection();
                LoopState::Continue
            }
            other => {
                self.toast = Some(format!("no handler for action: {other}"));
                LoopState::Continue
            }
        }
    }

    /// If the current overlay is a live-filter source (search prompt),
    /// push its current filter down to the active view so it applies
    /// live as the user types.
    fn apply_overlay_live_filter(&mut self) {
        let buffer = self
            .overlay
            .as_ref()
            .and_then(|o| o.live_filter_buffer())
            .map(|s| s.to_string());
        if let Some(buf) = buffer {
            self.current_view.set_filter(Filter::parse(&buf));
        }
    }

    fn copy_kubectl_for_selection(&mut self) {
        let Some(describe) = self.actions.by_id("describe") else {
            return;
        };
        match describe.kubectl_equivalent(self.current_view.as_ref()) {
            Some(cmd) => match crate::kubectl::copy_to_clipboard(&cmd) {
                Ok(()) => self.toast = Some(format!("copied: {cmd}")),
                Err(e) => self.toast = Some(format!("copy failed: {e}")),
            },
            None => self.toast = Some("no kubectl equivalent for selection".into()),
        }
    }

    /// Replace the currently active view.
    pub fn switch_view(&mut self, view: Box<dyn ResourceView>) {
        self.current_view = view;
    }

    /// Construct a fresh view for a given id. Returns `None` for
    /// unknown ids.
    fn view_for_id(id: &str) -> Option<Box<dyn ResourceView>> {
        Some(match id {
            "pods" => Box::new(PodsView::new()),
            "deployments" => Box::new(DeploymentsView::new()),
            "services" => Box::new(ServicesView::new()),
            "nodes" => Box::new(NodesView::new()),
            "events" => Box::new(EventsView::new()),
            "configmaps" => Box::new(ConfigMapsView::new()),
            "secrets" => Box::new(SecretsView::new()),
            "namespaces" => Box::new(NamespacesView::new()),
            _ => return None,
        })
    }

    /// Pure, top-level key handler.
    pub fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }

        // Clear toast on any keystroke.
        self.toast = None;

        // Overlay (palette/search) swallows keys.
        if self.overlay.is_some() {
            let result = self
                .overlay
                .as_mut()
                .map(|o| o.handle_key(key))
                .unwrap_or(OverlayResult::KeepOpen);
            // Live-update the view filter on every keystroke for the search prompt.
            self.apply_overlay_live_filter();
            match result {
                OverlayResult::KeepOpen => {}
                OverlayResult::Close => {
                    // On close, the live filter is already set (or empty for esc).
                    self.overlay = None;
                }
                OverlayResult::Invoke(id) => {
                    self.overlay = None;
                    return self.invoke_action(&id);
                }
                OverlayResult::SwitchView(id) => {
                    self.overlay = None;
                    if let Some(idx_str) = id.strip_prefix("wf:") {
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            self.run_workflow(idx);
                        } else {
                            self.toast = Some(format!("bad workflow id: {id}"));
                        }
                    } else if let Some(v) = Self::view_for_id(&id) {
                        self.current_view = v;
                        self.history.record(&id, None);
                    } else {
                        self.toast = Some(format!("no view for id: {id}"));
                    }
                }
            }
            return LoopState::Continue;
        }

        // Action panes swallow keys when open.
        if self.describe_pane.is_open() {
            self.describe_pane.handle_key(key);
            return LoopState::Continue;
        }
        if self.logs_pane.is_open() {
            self.logs_pane.handle_key(key);
            return LoopState::Continue;
        }

        if self.port_forward_prompt.is_some() {
            self.handle_port_forward_prompt_key(key);
            return LoopState::Continue;
        }

        // Ctrl+P opens the command palette.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.open_palette();
            return LoopState::Continue;
        }

        // Ctrl+R toggles read-only (forbidden in Prod).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            if self.environment == Environment::Prod {
                self.toast = Some(
                    "read-only is forced for prod contexts; restart with --rw to override".into(),
                );
            } else {
                self.read_only = !self.read_only;
                let state = if self.read_only { "ON" } else { "OFF" };
                self.toast = Some(format!("read-only: {state}"));
            }
            return LoopState::Continue;
        }

        // / opens the search prompt.
        if key.code == KeyCode::Char('/') {
            self.overlay = Some(Box::new(SearchPrompt::new()));
            return LoopState::Continue;
        }

        if self.command.is_active() {
            match self.command.handle_key(key) {
                CommandAction::None | CommandAction::Cancel => {}
                CommandAction::SwitchTo(id) => match Self::view_for_id(&id) {
                    Some(v) => {
                        self.current_view = v;
                        self.history.record(&id, None);
                    }
                    None => self.toast = Some(format!("no view for id: {id}")),
                },
                CommandAction::UnknownAlias(alias) => {
                    self.toast = Some(format!("unknown alias: :{alias}"));
                }
            }
            return LoopState::Continue;
        }

        match key.code {
            KeyCode::Char(':') => {
                self.command.activate();
                LoopState::Continue
            }
            KeyCode::Char('q') | KeyCode::Esc => LoopState::Quit,
            KeyCode::Char('d') | KeyCode::Char('y') => {
                if let Some((title, yaml)) = self.current_view.selected_yaml() {
                    self.describe_pane.open(title, yaml);
                } else {
                    self.toast = Some("nothing selected".into());
                }
                LoopState::Continue
            }
            KeyCode::Char('l') => {
                self.open_logs_for_selection();
                LoopState::Continue
            }
            KeyCode::Char('s') => {
                self.exec_into_selection();
                LoopState::Continue
            }
            KeyCode::Char('f') => {
                self.start_port_forward_prompt();
                LoopState::Continue
            }
            KeyCode::Char('e') => {
                self.edit_selection_yaml();
                LoopState::Continue
            }
            KeyCode::Char('K') => {
                self.copy_kubectl_for_selection();
                LoopState::Continue
            }
            KeyCode::Char('H') => {
                self.open_history_palette();
                LoopState::Continue
            }
            KeyCode::Char('r') => {
                // Resolving relationships needs registry snapshots
                // (async). Defer to the run loop, which runs the
                // resolver between input poll and draw.
                self.pending_open_relationships = true;
                LoopState::Continue
            }
            KeyCode::Char('W') => {
                self.open_workflows_palette();
                LoopState::Continue
            }
            _ => self.current_view.handle_key(key),
        }
    }

    fn edit_selection_yaml(&mut self) {
        if self.read_only {
            self.toast = Some("read-only mode: edit disabled (Ctrl+R to toggle)".into());
            return;
        }
        // Refuse to edit Secrets — the selected_yaml() for SecretsView
        // returns redacted YAML, and applying that would clobber the
        // real values. For v1, point users to kubectl.
        let Some(key) = self.current_view.selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        if key.kind == "Secret" {
            self.toast = Some(
                "editing Secrets via cruster is disabled in v1 — use kubectl edit secret ..."
                    .into(),
            );
            return;
        }
        let Some((_, yaml)) = self.current_view.selected_yaml() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        match crate::actions::yaml_edit::edit_and_apply(&yaml) {
            Ok(_) => self.toast = Some("applied".into()),
            Err(e) => self.toast = Some(format!("edit failed: {e}")),
        }
    }

    fn start_port_forward_prompt(&mut self) {
        if self.read_only {
            self.toast = Some("read-only mode: port-forward disabled (Ctrl+R to toggle)".into());
            return;
        }
        let Some(key) = self.current_view.selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        if key.kind != "Pod" {
            self.toast = Some(format!(
                "port-forward only supported for pods (selected: {})",
                key.kind
            ));
            return;
        }
        self.port_forward_prompt = Some(PortForwardPrompt {
            pod_key: key,
            buffer: String::new(),
        });
    }

    fn handle_port_forward_prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.port_forward_prompt.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.port_forward_prompt = None;
            }
            KeyCode::Enter => {
                let prompt = self.port_forward_prompt.take().unwrap();
                match PortForward::start(prompt.pod_key.clone(), prompt.buffer.clone()) {
                    Ok(pf) => {
                        self.toast = Some(format!(
                            "forwarded {} → pod/{}:{}",
                            pf.mapping, prompt.pod_key.name, pf.mapping
                        ));
                        self.port_forwards.add(pf);
                    }
                    Err(e) => {
                        self.toast = Some(format!("port-forward failed: {e}"));
                    }
                }
            }
            KeyCode::Backspace => {
                prompt.buffer.pop();
            }
            KeyCode::Char(c) => {
                prompt.buffer.push(c);
            }
            _ => {}
        }
    }

    fn exec_into_selection(&mut self) {
        let Some(key) = self.current_view.selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        if !self.current_view.selected_can_exec() {
            self.toast = Some(format!(
                "exec unavailable: {} is not a running pod with a ready container",
                key.name
            ));
            return;
        }
        if let Err(e) = crate::actions::exec::exec_into(&key) {
            self.toast = Some(format!("exec failed: {e}"));
        }
    }

    fn open_logs_for_selection(&mut self) {
        let Some(key) = self.current_view.selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        if key.kind != "Pod" {
            self.toast = Some(format!(
                "logs are only available for pods (selected: {})",
                key.kind
            ));
            return;
        }
        let Some(client) = self.client.clone() else {
            self.toast = Some("no kube client (test mode?)".into());
            return;
        };
        self.logs_pane.open(client, &key);
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut terminal = init_terminal()?;
        let result = self.run_loop(&mut terminal).await;
        restore_terminal()?;
        result
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> anyhow::Result<()> {
        loop {
            self.current_view.refresh(&self.registry).await;
            if self.pending_open_relationships {
                self.pending_open_relationships = false;
                self.open_relationships_overlay().await;
            }
            terminal.draw(|f| self.render_full(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key) == LoopState::Quit {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn open_relationships_overlay(&mut self) {
        let Some(key) = self.current_view.selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        let related = cruster_kube::related(&key, &self.registry).await;
        if related.is_empty() {
            self.toast = Some(format!(
                "no relationships discovered for {}/{}",
                key.kind, key.name
            ));
            return;
        }
        self.overlay = Some(Box::new(
            crate::overlays::relationships::RelationshipsOverlay::new(key.to_string(), related),
        ));
    }

    fn render_full(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if self.describe_pane.is_open() || self.logs_pane.is_open() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            self.current_view.render(frame);
            if self.describe_pane.is_open() {
                self.describe_pane.render(frame, chunks[1]);
            } else {
                self.logs_pane.render(frame, chunks[1]);
            }
        } else {
            self.current_view.render(frame);
        }
        self.render_safety_badge(frame);
        self.render_action_footer(frame);
        self.render_overlay(frame);
        if let Some(overlay) = &self.overlay {
            overlay.render(frame, frame.area());
        }
    }

    fn render_safety_badge(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let mode = if self.read_only { "ro" } else { "rw" };
        let label = format!(" [{}] {} {} ", self.context, self.environment, mode);
        let color = match self.environment {
            Environment::Prod => Color::Red,
            Environment::Staging => Color::Yellow,
            Environment::Dev => Color::Green,
            Environment::Local => Color::Cyan,
            Environment::Unknown => Color::DarkGray,
        };
        let bar = Paragraph::new(label).style(Style::default().bg(color).fg(Color::Black));
        let rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }

    fn render_action_footer(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.height < 2 {
            return;
        }
        let hints: Vec<String> = self
            .actions
            .applicable(self.current_view.as_ref())
            .map(|a| format_action_hint(a.key(), a.label()))
            .collect();
        if hints.is_empty() {
            return;
        }
        let mut line = String::new();
        for p in hints {
            let sep = if line.is_empty() { "" } else { "  " };
            if line.len() + sep.len() + p.len() > area.width as usize {
                if line.len() < area.width as usize {
                    line.push('…');
                }
                break;
            }
            line.push_str(sep);
            line.push_str(&p);
        }
        let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
        let rect = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(2),
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }

    fn render_overlay(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let bottom = Rect {
            x: area.x,
            y: area.y + area.height.saturating_sub(1),
            width: area.width,
            height: 1,
        };
        // Suppress drawing if no overlay is active — the action footer
        // is above and is always visible.
        if let Some(prompt) = &self.port_forward_prompt {
            let line = format!(
                "port-forward pod/{}: {}_ (esc cancels)",
                prompt.pod_key.name, prompt.buffer
            );
            let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
            frame.render_widget(bar, bottom);
        } else if self.command.is_active() {
            let line = format!(":{}", self.command.buffer());
            let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
            frame.render_widget(bar, bottom);
        } else if let Some(msg) = &self.toast {
            let bar = Paragraph::new(msg.clone()).style(Style::default().bg(Color::Red));
            frame.render_widget(bar, bottom);
        } else if !self.port_forwards.is_empty() {
            let line = format!(" port-forwards: {} active ", self.port_forwards.len());
            let bar = Paragraph::new(line).style(Style::default().bg(Color::Blue));
            frame.render_widget(bar, bottom);
        }
    }
}

/// Resolve the current kubeconfig context name. Falls back to
/// "unknown" if kubeconfig can't be read.
fn load_current_context() -> String {
    let Ok(cfg) = kube::config::Kubeconfig::read() else {
        return "unknown".into();
    };
    cfg.current_context.unwrap_or_else(|| "unknown".into())
}

fn format_action_hint(key: KeyCode, label: &str) -> String {
    let k = match key {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        other => format!("{other:?}"),
    };
    format!("[{k}] {label}")
}

fn init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal() -> anyhow::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
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

    fn app() -> App {
        App::new(StoreRegistry::new(), None)
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('q'))), LoopState::Quit);
    }

    #[test]
    fn esc_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Esc)), LoopState::Quit);
    }

    #[test]
    fn unknown_key_continues_via_view() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('x'))), LoopState::Continue);
    }

    #[test]
    fn key_release_is_ignored_at_app_level() {
        let mut a = app();
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(a.handle_key(release), LoopState::Continue);
    }

    #[test]
    fn colon_activates_command_mode() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char(':')));
        assert!(a.command.is_active());
    }

    #[test]
    fn typing_in_command_mode_does_not_quit_on_q() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char(':')));
        let state = a.handle_key(press(KeyCode::Char('q')));
        assert_eq!(state, LoopState::Continue);
        assert_eq!(a.command.buffer(), "q");
    }

    #[test]
    fn enter_with_known_alias_switches_view() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char(':')));
        for ch in "deploy".chars() {
            a.handle_key(press(KeyCode::Char(ch)));
        }
        a.handle_key(press(KeyCode::Enter));
        assert_eq!(a.current_view.id(), "deployments");
    }

    #[test]
    fn unknown_alias_sets_toast() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char(':')));
        for ch in "wat".chars() {
            a.handle_key(press(KeyCode::Char(ch)));
        }
        a.handle_key(press(KeyCode::Enter));
        assert!(a.toast.is_some());
    }

    #[test]
    fn d_with_no_selection_shows_toast() {
        let mut a = app();
        let _ = a.handle_key(press(KeyCode::Char('d')));
        assert!(!a.describe_pane.is_open());
        assert!(a.toast.is_some());
    }

    #[test]
    fn describe_pane_swallows_keys_when_open() {
        let mut a = app();
        a.describe_pane.open("test", "yaml");
        // 'q' should not quit when describe pane is open
        assert_eq!(a.handle_key(press(KeyCode::Char('q'))), LoopState::Continue);
        // Esc closes the pane
        let _ = a.handle_key(press(KeyCode::Esc));
        assert!(!a.describe_pane.is_open());
    }

    #[test]
    fn l_with_no_selection_shows_toast() {
        let mut a = app();
        let _ = a.handle_key(press(KeyCode::Char('l')));
        assert!(!a.logs_pane.is_open());
        assert!(a.toast.is_some());
    }

    #[test]
    fn footer_renders_applicable_actions() {
        let a = app();
        // No pods → only actions that don't require a selection apply.
        let labels: Vec<&'static str> = a
            .actions
            .applicable(a.current_view.as_ref())
            .map(|act| act.label())
            .collect();
        assert!(labels.contains(&"Switch kind"));
        assert!(labels.contains(&"Quit"));
        // Describe needs a selection; with an empty store, it shouldn't apply.
        assert!(!labels.contains(&"Describe"));
    }

    #[test]
    fn format_hint_renders_char_and_label() {
        assert_eq!(
            format_action_hint(KeyCode::Char('d'), "Describe"),
            "[d] Describe"
        );
        assert_eq!(format_action_hint(KeyCode::Esc, "Cancel"), "[esc] Cancel");
    }
}
