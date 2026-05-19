//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use cruster_core::{tier::Tier, Environment};
use cruster_kube::StoreRegistry;
use kube::Client;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use ratatui::Terminal;

use crate::actions::describe::DescribePane;
use crate::actions::logs::LogsPane;
use crate::actions::port_forward::{PortForward, PortForwards};
use crate::command::{CommandAction, CommandLine};
use crate::dashboard::{DashboardConfig, Pin};
use crate::overlay::{Overlay, OverlayResult};
#[allow(unused_imports)]
use crate::overlays::palette::{EntryKind, Palette, PaletteEntry};
use crate::overlays::search::{Filter, SearchPrompt};
use crate::prompts::PromptDef;
use crate::view::ResourceView;
use crate::views::configmaps::ConfigMapsView;
use crate::views::daemonsets::DaemonSetsView;
use crate::views::dashboard::DashboardView;
use crate::views::deployments::DeploymentsView;
use crate::views::events::EventsView;
use crate::views::namespaces::NamespacesView;
use crate::views::nodes::NodesView;
use crate::views::pods::PodsView;
use crate::views::secrets::SecretsView;
use crate::views::services::ServicesView;
use crate::views::statefulsets::StatefulSetsView;
use cruster_core::ResourceKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

/// Which pane currently receives non-overlay keystrokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    View,
    Describe,
    Logs,
}

pub struct App {
    registry: StoreRegistry,
    client: Option<Client>,
    /// Currently active view. `None` when the dashboard is active (use
    /// `dashboard` field instead).
    current_view: Option<Box<dyn ResourceView>>,
    /// Persisted dashboard view — never dropped, so per-pin sparkline
    /// history survives `dashboard -> pods -> dashboard`.
    dashboard: DashboardView,
    actions: crate::action::ActionRegistry,
    command: CommandLine,
    describe_pane: DescribePane,
    logs_pane: LogsPane,
    port_forwards: PortForwards,
    overlay: Option<Box<dyn Overlay>>,
    history: crate::history::History,
    loaded_workflows: Vec<crate::workflows::Workflow>,
    context: String,
    environment: Environment,
    read_only: bool,
    pending_open_relationships: bool,
    theme: crate::theme::Theme,
    tier: Tier,
    keymap: crate::keymap::Keymap,
    layout: crate::layout::Layout,
    pane_focus: PaneFocus,
    toast: Option<String>,
    prompts: Vec<PromptDef>,
    /// Set when the user presses the prompt leader (`P`). The next
    /// keystroke is consumed as the prompt-trigger character rather
    /// than dispatched through the keymap.
    prompt_leader_armed: bool,
    /// Deferred work: the next async tick of `run_loop` picks this up,
    /// builds the snapshot, renders the matching prompt template, and
    /// copies the result to the clipboard.
    pending_prompt_trigger: Option<char>,
    /// Deferred work: `E` was pressed — build a diagnostic export on
    /// the next async tick.
    pending_export: bool,
    /// In-flight `kubectl delete`: a tokio task is awaiting kubectl
    /// in the background; the run_loop polls this each tick and
    /// drains it once kubectl finishes. None when no delete is
    /// pending. The String inside the result is the human label so
    /// the toast can name what got deleted.
    delete_in_flight: Option<(String, tokio::sync::oneshot::Receiver<Result<(), String>>)>,
}

impl App {
    pub fn new(registry: StoreRegistry, client: Option<Client>) -> Self {
        let context = load_current_context();
        let environment = crate::safety::SafetyConfig::load_or_default().classify(&context);
        let read_only = environment.requires_confirmation();
        Self {
            registry,
            client,
            current_view: None,
            dashboard: DashboardView::new(),
            actions: crate::action_shipped::default_registry(),
            command: CommandLine::new(),
            describe_pane: DescribePane::new(),
            logs_pane: LogsPane::new(),
            port_forwards: PortForwards::new(),
            overlay: None,
            history: {
                let mut h = crate::history::History::with_capacity(200);
                h.record("dashboard", None);
                h
            },
            loaded_workflows: Vec::new(),
            context,
            environment,
            read_only,
            pending_open_relationships: false,
            theme: crate::theme::Theme::terminal_default(),
            tier: cruster_core::license::load_tier_or_free(),
            keymap: crate::keymap::Keymap::for_preset(
                crate::keymap::KeymapConfig::load_or_default().preset,
            ),
            layout: crate::layout::Layout::default(),
            pane_focus: PaneFocus::View,
            toast: None,
            prompts: crate::prompts::load_all(),
            prompt_leader_armed: false,
            pending_prompt_trigger: None,
            pending_export: false,
            delete_in_flight: None,
        }
    }

    /// Returns a reference to the active view (dashboard or other).
    fn active_view(&self) -> &dyn ResourceView {
        match &self.current_view {
            Some(v) => v.as_ref(),
            None => &self.dashboard,
        }
    }

    /// Returns a mutable reference to the active view (dashboard or other).
    fn active_view_mut(&mut self) -> &mut dyn ResourceView {
        match &mut self.current_view {
            Some(v) => v.as_mut(),
            None => &mut self.dashboard,
        }
    }

    fn cycle_pane_focus(&mut self) {
        let mut candidates = vec![PaneFocus::View];
        if self.describe_pane.is_open() {
            candidates.push(PaneFocus::Describe);
        }
        if self.logs_pane.is_open() {
            candidates.push(PaneFocus::Logs);
        }
        if candidates.len() < 2 {
            return;
        }
        let current = candidates
            .iter()
            .position(|f| *f == self.pane_focus)
            .unwrap_or(0);
        self.pane_focus = candidates[(current + 1) % candidates.len()];
    }

    /// Normalise focus so it never points to a closed pane.
    fn normalise_pane_focus(&mut self) {
        match self.pane_focus {
            PaneFocus::Describe if !self.describe_pane.is_open() => {
                self.pane_focus = PaneFocus::View;
            }
            PaneFocus::Logs if !self.logs_pane.is_open() => {
                self.pane_focus = PaneFocus::View;
            }
            _ => {}
        }
    }

    fn dispatch_semantic(&mut self, action: crate::keymap::SemanticAction) -> LoopState {
        use crate::keymap::SemanticAction as S;
        match action {
            S::Quit => LoopState::Quit,
            S::MoveUp => {
                self.synthetic_key_to_view(KeyCode::Char('k'));
                LoopState::Continue
            }
            S::MoveDown => {
                self.synthetic_key_to_view(KeyCode::Char('j'));
                LoopState::Continue
            }
            S::MoveTop => {
                self.synthetic_key_to_view(KeyCode::Char('g'));
                LoopState::Continue
            }
            S::MoveBottom => {
                self.synthetic_key_to_view(KeyCode::Char('G'));
                LoopState::Continue
            }
            S::OpenPalette => {
                self.open_palette();
                LoopState::Continue
            }
            S::OpenSearch => {
                self.overlay = Some(Box::new(SearchPrompt::new()));
                LoopState::Continue
            }
            S::OpenCommandMode => {
                self.command.activate();
                LoopState::Continue
            }
            S::OpenHistory => {
                self.open_history_palette();
                LoopState::Continue
            }
            S::OpenWorkflows => {
                self.open_workflows_palette();
                LoopState::Continue
            }
            S::OpenThemes => {
                self.open_themes_palette();
                LoopState::Continue
            }
            S::OpenRelationships => {
                self.pending_open_relationships = true;
                LoopState::Continue
            }
            S::Describe => {
                if let Some((title, yaml)) = self.active_view().selected_yaml() {
                    self.describe_pane.open(title, yaml);
                    self.pane_focus = PaneFocus::Describe;
                } else {
                    self.toast = Some("nothing selected".into());
                }
                LoopState::Continue
            }
            S::Logs => {
                self.open_logs_for_selection();
                if self.logs_pane.is_open() {
                    self.pane_focus = PaneFocus::Logs;
                }
                LoopState::Continue
            }
            S::Exec => {
                self.exec_into_selection();
                LoopState::Continue
            }
            S::PortForward => {
                self.start_port_forward_prompt();
                LoopState::Continue
            }
            S::EditYaml => {
                self.edit_selection_yaml();
                LoopState::Continue
            }
            S::CopyKubectl => {
                self.copy_kubectl_for_selection();
                LoopState::Continue
            }
            S::ToggleReadOnly => {
                if self.environment == Environment::Prod {
                    self.toast = Some(
                        "read-only is forced for prod contexts; restart with --rw to override"
                            .into(),
                    );
                } else {
                    self.read_only = !self.read_only;
                    let state = if self.read_only { "ON" } else { "OFF" };
                    self.toast = Some(format!("read-only: {state}"));
                }
                LoopState::Continue
            }
            S::LayoutSingle => {
                self.layout = crate::layout::Layout::Single;
                self.toast = Some("layout: single".into());
                LoopState::Continue
            }
            S::LayoutTriplet => {
                self.layout = crate::layout::Layout::Triplet;
                self.toast = Some("layout: triplet".into());
                LoopState::Continue
            }
            S::LayoutIncident => {
                self.layout = crate::layout::Layout::Incident;
                self.toast = Some("layout: incident".into());
                LoopState::Continue
            }
            S::OpenPromptLeader => {
                if !self.tier.has_pro() {
                    self.toast = Some("prompt actions are a Pro feature".into());
                    return LoopState::Continue;
                }
                if self.prompts.is_empty() {
                    self.toast = Some("no prompts configured".into());
                    return LoopState::Continue;
                }
                self.prompt_leader_armed = true;
                self.toast = Some(format!(
                    "prompt: press {} (or esc)",
                    prompt_trigger_hints(&self.prompts)
                ));
                LoopState::Continue
            }
            S::ExportDiagnostic => {
                if !self.tier.has_pro() {
                    self.toast = Some("diagnostic export is a Pro feature".into());
                    return LoopState::Continue;
                }
                if self.active_view().selected_key().is_none() {
                    self.toast = Some("nothing selected to export".into());
                    return LoopState::Continue;
                }
                self.pending_export = true;
                LoopState::Continue
            }
            S::PinToDashboard => {
                self.pin_current_selection();
                LoopState::Continue
            }
            S::Delete => {
                self.start_delete_prompt();
                LoopState::Continue
            }
            S::OpenHelp => {
                self.open_help_overlay();
                LoopState::Continue
            }
        }
    }

    fn pin_current_selection(&mut self) {
        let Some(key) = self.active_view().selected_key() else {
            self.toast = Some("nothing selected to pin".into());
            return;
        };
        if self.active_view().id() == "dashboard" {
            self.toast = Some("already on dashboard — press 'a' from a list view".into());
            return;
        }
        let pin = Pin::from_key(&key);
        let mut cfg = DashboardConfig::load_or_default();
        if !cfg.add(pin.clone()) {
            self.toast = Some(format!("already pinned: {}", pin.label()));
            return;
        }
        match cfg.save() {
            Ok(()) => self.toast = Some(format!("pinned: {}", pin.label())),
            Err(e) => self.toast = Some(format!("pin save failed: {e}")),
        }
    }

    fn synthetic_key_to_view(&mut self, code: KeyCode) {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        self.active_view_mut().handle_key(key);
    }

    fn open_themes_palette(&mut self) {
        let entries: Vec<PaletteEntry> = crate::theme::Theme::bundled_names()
            .iter()
            .map(|name| PaletteEntry {
                id: format!("theme:{name}"),
                label: format!("Theme: {name}"),
                kind: EntryKind::View,
            })
            .collect();
        self.overlay = Some(Box::new(Palette::new(entries)));
    }

    fn apply_theme(&mut self, name: &str) {
        if name != "terminal" && !self.tier.has_pro() {
            self.toast = Some(format!("theme '{name}' is a Pro feature"));
            return;
        }
        match crate::theme::Theme::embedded(name) {
            Some(t) => {
                self.theme = t;
                self.toast = Some(format!("theme: {name}"));
            }
            None => {
                self.toast = Some(format!("unknown theme: {name}"));
            }
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
                crate::workflows::Step::SwitchView(id) => {
                    self.switch_to_view_id(id);
                }
                crate::workflows::Step::SetFilter(query) => {
                    self.active_view_mut().set_filter(Filter::parse(query));
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
            "dashboard",
            "pods",
            "deployments",
            "statefulsets",
            "daemonsets",
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
                if let Some((title, yaml)) = self.active_view().selected_yaml() {
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
            self.active_view_mut().set_filter(Filter::parse(&buf));
        }
    }

    fn copy_kubectl_for_selection(&mut self) {
        let Some(describe) = self.actions.by_id("describe") else {
            return;
        };
        match describe.kubectl_equivalent(self.active_view()) {
            Some(cmd) => match crate::kubectl::copy_to_clipboard(&cmd) {
                Ok(()) => self.toast = Some(format!("copied: {cmd}")),
                Err(e) => self.toast = Some(format!("copy failed: {e}")),
            },
            None => self.toast = Some("no kubectl equivalent for selection".into()),
        }
    }

    /// Replace the currently active view.
    pub fn switch_view(&mut self, view: Box<dyn ResourceView>) {
        self.current_view = Some(view);
    }

    /// Switch to a view by id. Dashboard is handled specially to reuse
    /// the cached instance (preserving sparkline history).
    fn switch_to_view_id(&mut self, id: &str) {
        if id == "dashboard" {
            self.dashboard.reload_pins();
            self.current_view = None;
            self.history.record(id, None);
        } else if let Some(v) = Self::view_for_id(id) {
            self.current_view = Some(v);
            self.history.record(id, None);
        } else {
            self.toast = Some(format!("no view for id: {id}"));
        }
    }

    /// Construct a fresh view for a given id (except dashboard). Returns
    /// `None` for unknown ids. Dashboard returns `None` since it's cached
    /// on App; use `switch_to_view_id` for that.
    fn view_for_id(id: &str) -> Option<Box<dyn ResourceView>> {
        Some(match id {
            "dashboard" => return None,
            "pods" => Box::new(PodsView::new()),
            "deployments" => Box::new(DeploymentsView::new()),
            "statefulsets" => Box::new(StatefulSetsView::new()),
            "daemonsets" => Box::new(DaemonSetsView::new()),
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

        // Prompt leader armed: this keystroke chooses the prompt (or
        // cancels). Consume it before doing anything else so it can't
        // bleed into a view binding (e.g. armed → user presses `d`
        // shouldn't open the describe pane).
        if self.prompt_leader_armed {
            self.prompt_leader_armed = false;
            self.toast = None;
            match key.code {
                KeyCode::Char(c) => {
                    self.pending_prompt_trigger = Some(c);
                }
                _ => {
                    self.toast = Some("prompt cancelled".into());
                }
            }
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
                    if id == "port-forward-submit" {
                        // Pull the overlay's payload before dropping it.
                        let payload = self.overlay.as_ref().and_then(|o| o.port_forward_payload());
                        self.overlay = None;
                        if let Some((pod_key, mapping)) = payload {
                            self.submit_port_forward(pod_key, mapping);
                        }
                        return LoopState::Continue;
                    }
                    if id == "delete-submit" {
                        let payload = self.overlay.as_ref().and_then(|o| o.delete_payload());
                        self.overlay = None;
                        if let Some((key, policy, force)) = payload {
                            self.submit_delete(key, policy, force);
                        }
                        return LoopState::Continue;
                    }
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
                    } else if let Some(name) = id.strip_prefix("theme:") {
                        self.apply_theme(name);
                    } else {
                        self.switch_to_view_id(&id);
                    }
                }
            }
            return LoopState::Continue;
        }

        // Make sure focus is valid (panes may have closed since last tick).
        self.normalise_pane_focus();

        // Tab cycles focus across view + open panes.
        if key.code == KeyCode::Tab {
            self.cycle_pane_focus();
            return LoopState::Continue;
        }

        // Route to focused pane first. The pane returns `true` if it
        // consumed the key (Esc / scroll chords / grep typing); if it
        // returns `false`, the chord falls through to the keymap so
        // global commands like `D` (delete) and `?` (help) still
        // fire while a side pane is in focus.
        let consumed_by_pane = match self.pane_focus {
            PaneFocus::Describe if self.describe_pane.is_open() => {
                self.describe_pane.handle_key(key)
            }
            PaneFocus::Logs if self.logs_pane.is_open() => self.logs_pane.handle_key(key),
            _ => false,
        };
        if consumed_by_pane {
            return LoopState::Continue;
        }

        if self.command.is_active() {
            match self.command.handle_key(key) {
                CommandAction::None | CommandAction::Cancel => {}
                CommandAction::SwitchTo(id) => {
                    self.switch_to_view_id(&id);
                }
                CommandAction::UnknownAlias(alias) => {
                    self.toast = Some(format!("unknown alias: :{alias}"));
                }
            }
            return LoopState::Continue;
        }

        // Route through the user's keymap preset. If the chord is bound
        // to a semantic action, dispatch it. Otherwise fall through to
        // the current view (handles arrow keys etc).
        if let Some(action) = self.keymap.resolve(key.code, key.modifiers) {
            return self.dispatch_semantic(action);
        }

        let state = self.active_view_mut().handle_key(key);
        if let Some(id) = self.active_view_mut().take_pending_view_switch() {
            self.switch_to_view_id(id);
        }
        if let Some(msg) = self.active_view_mut().take_pending_toast() {
            self.toast = Some(msg);
        }
        state
    }

    fn edit_selection_yaml(&mut self) {
        if self.read_only {
            self.toast = Some("read-only mode: edit disabled (Ctrl+R to toggle)".into());
            return;
        }
        // Refuse to edit Secrets — the selected_yaml() for SecretsView
        // returns redacted YAML, and applying that would clobber the
        // real values. For v1, point users to kubectl.
        let Some(key) = self.active_view().selected_key() else {
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
        let Some((_, yaml)) = self.active_view().selected_yaml() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        match crate::actions::yaml_edit::edit_and_apply(&yaml) {
            Ok(_) => self.toast = Some("applied".into()),
            Err(e) => self.toast = Some(format!("edit failed: {e}")),
        }
    }

    fn start_delete_prompt(&mut self) {
        if self.read_only {
            self.toast = Some("read-only mode: delete disabled (Ctrl+R to toggle)".into());
            return;
        }
        let Some(key) = self.active_view().selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        self.overlay = Some(Box::new(crate::overlays::delete::DeleteOverlay::new(key)));
    }

    fn open_help_overlay(&mut self) {
        self.overlay = Some(Box::new(crate::overlays::help::HelpOverlay::new(
            self.keymap.clone(),
        )));
    }

    fn submit_delete(
        &mut self,
        key: cruster_core::ResourceKey,
        policy: crate::actions::delete::PropagationPolicy,
        force: bool,
    ) {
        let label = match key.namespace.as_deref() {
            Some(ns) => format!("{}/{} in {}", key.kind.to_lowercase(), key.name, ns),
            None => format!("{}/{}", key.kind.to_lowercase(), key.name),
        };
        if self.delete_in_flight.is_some() {
            self.toast =
                Some("another delete is still in flight — try again when it finishes".into());
            return;
        }
        // Spawn kubectl in the background. The run_loop polls the
        // receiver each tick and toasts when the result lands. The
        // event loop keeps drawing while kubectl runs, so a slow
        // delete (Foreground propagation, unreachable apiserver)
        // can never freeze the UI.
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result = crate::actions::delete::run_delete(key, policy, force)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
        self.toast = Some(format!("deleting {label}…"));
        self.delete_in_flight = Some((label, rx));
    }

    /// Drained once per run_loop tick. If the in-flight kubectl
    /// finished, set a result toast and clear the slot.
    fn poll_delete_in_flight(&mut self) {
        let Some((label, mut rx)) = self.delete_in_flight.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                self.toast = Some(format!("deleted {label}"));
            }
            Ok(Err(msg)) => {
                self.toast = Some(format!("delete {label} failed: {msg}"));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                // Still running — put it back so the next tick polls
                // it again.
                self.delete_in_flight = Some((label, rx));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.toast = Some(format!("delete {label} failed: worker dropped"));
            }
        }
    }

    fn start_port_forward_prompt(&mut self) {
        if self.read_only {
            self.toast = Some("read-only mode: port-forward disabled (Ctrl+R to toggle)".into());
            return;
        }
        let Some(key) = self.active_view().selected_key() else {
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
        self.overlay = Some(Box::new(
            crate::overlays::port_forward::PortForwardOverlay::new(key),
        ));
    }

    fn submit_port_forward(&mut self, pod_key: ResourceKey, mapping: String) {
        match PortForward::start(pod_key.clone(), mapping.clone()) {
            Ok(pf) => {
                self.toast = Some(format!("forwarded {} → pod/{}", pf.mapping, pod_key.name));
                self.port_forwards.add(pf);
            }
            Err(e) => {
                self.toast = Some(format!("port-forward failed: {e}"));
            }
        }
    }

    fn exec_into_selection(&mut self) {
        let Some(key) = self.active_view().selected_key() else {
            self.toast = Some("nothing selected".into());
            return;
        };
        if !self.active_view().selected_can_exec() {
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
        let Some(key) = self.active_view().selected_key() else {
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
            match &mut self.current_view {
                Some(v) => v.refresh(&self.registry).await,
                None => self.dashboard.refresh(&self.registry).await,
            }
            if self.pending_open_relationships {
                self.pending_open_relationships = false;
                self.open_relationships_overlay().await;
            }
            if let Some(trigger) = self.pending_prompt_trigger.take() {
                self.trigger_prompt(trigger).await;
            }
            if self.pending_export {
                self.pending_export = false;
                self.run_export().await;
            }
            self.poll_delete_in_flight();
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

    /// Build a `Snapshot` from current app state. Events are filtered
    /// to those that involve the currently selected resource. Logs are
    /// populated from the logs pane buffer iff it's open and targeting
    /// the same pod.
    async fn build_snapshot(&self) -> cruster_core::context::Snapshot {
        use cruster_core::context::{ClusterContext, EventSummary, ResourceContext, Snapshot};

        let cluster = ClusterContext {
            name: self.context.clone(),
            context: self.context.clone(),
            environment: self.environment.to_string(),
        };

        let mut resource = None;
        let mut events: Vec<EventSummary> = Vec::new();
        let selected = self.active_view().selected_key();
        if let Some(key) = &selected {
            let raw_yaml = self
                .active_view()
                .selected_yaml()
                .map(|(_, y)| y)
                .unwrap_or_default();
            resource = Some(ResourceContext {
                key: key.clone(),
                status_summary: String::new(),
                age: String::new(),
                raw_yaml,
            });

            let all = self.registry.events.snapshot().await;
            for (_, e) in all {
                let kind_match = e.involved_object.kind.as_deref() == Some(key.kind.as_str());
                let name_match = e.involved_object.name.as_deref() == Some(key.name.as_str());
                if !(kind_match && name_match) {
                    continue;
                }
                let time = e
                    .last_timestamp
                    .as_ref()
                    .map(|t| t.0.to_rfc3339())
                    .unwrap_or_default();
                events.push(EventSummary {
                    time,
                    type_: e.type_.clone().unwrap_or_default(),
                    reason: e.reason.clone().unwrap_or_default(),
                    message: e.message.clone().unwrap_or_default(),
                });
            }
            // Most recent first, capped to keep templates compact.
            events.sort_by(|a, b| b.time.cmp(&a.time));
            events.truncate(20);
        }

        let logs = if self.logs_pane.is_open() {
            self.logs_pane.recent_lines(50)
        } else {
            Vec::new()
        };

        Snapshot {
            cluster,
            resource,
            events,
            logs,
            selection: None,
            pane: Some(format!("{:?}", self.pane_focus).to_lowercase()),
        }
    }

    async fn trigger_prompt(&mut self, trigger: char) {
        let prompt = self
            .prompts
            .iter()
            .find(|p| p.trigger() == Some(trigger))
            .cloned();
        let Some(prompt) = prompt else {
            self.toast = Some(format!("no prompt bound to '{trigger}'"));
            return;
        };
        let snapshot = self.build_snapshot().await;
        let rendered = match crate::prompts::render(&prompt.template, &snapshot) {
            Ok(s) => s,
            Err(e) => {
                self.toast = Some(format!("prompt '{}' render failed: {e}", prompt.name));
                return;
            }
        };
        match crate::kubectl::copy_to_clipboard(&rendered) {
            Ok(()) => {
                self.toast = Some(format!(
                    "copied prompt '{}' to clipboard ({} chars)",
                    prompt.name,
                    rendered.len()
                ));
            }
            Err(e) => {
                self.toast = Some(format!("prompt copy failed: {e}"));
            }
        }
    }

    async fn run_export(&mut self) {
        let snapshot = self.build_snapshot().await;
        let Some(resource) = snapshot.resource.as_ref() else {
            self.toast = Some("nothing selected to export".into());
            return;
        };
        let md = crate::export::build_markdown(&snapshot);
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let filename = format!(
            "{}-{}-{}.md",
            resource.key.kind.to_lowercase(),
            resource.key.name,
            ts
        );
        match std::fs::write(&filename, md) {
            Ok(()) => self.toast = Some(format!("wrote {filename}")),
            Err(e) => self.toast = Some(format!("export failed: {e}")),
        }
    }

    async fn open_relationships_overlay(&mut self) {
        let Some(key) = self.active_view().selected_key() else {
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
        let frame_area = frame.area();
        // Reserve 1 row at the top (safety badge) and 2 rows at the
        // bottom (action footer + overlay/toast strip). Whatever's
        // left is the view's territory — views that don't know to
        // dodge the chrome (dashboard's fixed-height pins band, etc.)
        // would otherwise have their bottom rows overwritten.
        let view_area = chrome_inset(frame_area);
        if self.describe_pane.is_open() || self.logs_pane.is_open() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(view_area);
            self.active_view().render(frame, chunks[0], &self.theme);
            if self.describe_pane.is_open() {
                self.describe_pane
                    .render(frame, chunks[1], self.pane_focus == PaneFocus::Describe);
            } else {
                self.logs_pane
                    .render(frame, chunks[1], self.pane_focus == PaneFocus::Logs);
            }
        } else {
            self.active_view().render(frame, view_area, &self.theme);
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
        if area.height == 0 || area.width == 0 {
            return;
        }
        let env = self.environment;
        let bg = match env {
            Environment::Prod => self.theme.env_band.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band.dev.as_ratatui(),
            Environment::Local => self.theme.env_band.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band.unknown.as_ratatui(),
        };
        let env_fg = match env {
            Environment::Prod => self.theme.env_band_fg.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band_fg.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band_fg.dev.as_ratatui(),
            Environment::Local => self.theme.env_band_fg.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band_fg.unknown.as_ratatui(),
        };
        let muted_fg = self.theme.muted_fg.as_ratatui();
        let mode_color = if self.read_only {
            self.theme.mode.ro.as_ratatui()
        } else {
            self.theme.mode.rw.as_ratatui()
        };
        let mode_text = if self.read_only { "RO" } else { "RW" };
        let env_text = self.environment.to_string().to_uppercase();
        let layout_text = self.layout.label().to_string();

        let sep_style = Style::default().bg(bg).fg(muted_fg);
        let value_style = Style::default()
            .bg(bg)
            .fg(env_fg)
            .add_modifier(Modifier::BOLD);
        let bracket_style = Style::default().bg(bg).fg(muted_fg);
        let mode_style = Style::default()
            .bg(bg)
            .fg(mode_color)
            .add_modifier(Modifier::BOLD);
        let muted_on_bg = Style::default().bg(bg).fg(muted_fg);

        // Width budgeting — keep the mode chip visible at all costs.
        // Reserved cost (always rendered): leading sep + sep after ctx
        // + mode chip + trailing sep after mode chip = 3+3+4+3 = 13.
        // Whatever remains gets spent on ctx (truncated if needed),
        // then optionally env (sep+env_w), then optionally layout
        // (sep+layout_w).
        let width = area.width as usize;
        let sep_w = 3usize;
        let mode_w = 4usize;
        let reserved = sep_w + sep_w + mode_w + sep_w;

        let sep = Span::styled(" │ ", sep_style);

        let spans: Vec<Span<'static>> = if width <= reserved {
            // Degenerate width: render only the mode chip.
            vec![
                Span::styled("[", bracket_style),
                Span::styled(mode_text.to_string(), mode_style),
                Span::styled("]", bracket_style),
            ]
        } else {
            let mut budget = width - reserved;
            let mut ctx_render = self.context.clone();
            let ctx_w = ctx_render.chars().count();
            if ctx_w <= budget {
                budget -= ctx_w;
            } else {
                ctx_render = truncate_with_ellipsis(&ctx_render, budget);
                budget = 0;
            }
            let env_w = env_text.chars().count();
            let layout_w = layout_text.chars().count();
            let include_env = budget >= sep_w + env_w;
            if include_env {
                budget -= sep_w + env_w;
            }
            let include_layout = budget >= sep_w + layout_w;

            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push(sep.clone());
            spans.push(Span::styled(ctx_render, value_style));
            spans.push(sep.clone());
            if include_env {
                spans.push(Span::styled(env_text, value_style));
                spans.push(sep.clone());
            }
            spans.push(Span::styled("[", bracket_style));
            spans.push(Span::styled(mode_text.to_string(), mode_style));
            spans.push(Span::styled("]", bracket_style));
            spans.push(sep.clone());
            if include_layout {
                spans.push(Span::styled(layout_text, muted_on_bg));
                spans.push(sep);
            }
            spans
        };

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().bg(bg));
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
            .applicable(self.active_view())
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
        let bar =
            Paragraph::new(line).style(Style::default().bg(self.theme.footer_bg.as_ratatui()));
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
        // is above and is always visible. The port-forward prompt is
        // now a modal overlay (rendered separately by the overlay
        // drawing pass).
        if self.command.is_active() {
            let line = format!(":{}", self.command.buffer());
            let bar =
                Paragraph::new(line).style(Style::default().bg(self.theme.command_bg.as_ratatui()));
            frame.render_widget(bar, bottom);
        } else if let Some(msg) = &self.toast {
            let bar = Paragraph::new(msg.clone())
                .style(Style::default().bg(self.theme.toast_bg.as_ratatui()));
            frame.render_widget(bar, bottom);
        } else if !self.port_forwards.is_empty() {
            let line = format!(" port-forwards: {} active ", self.port_forwards.len());
            let bar =
                Paragraph::new(line).style(Style::default().bg(self.theme.search_bg.as_ratatui()));
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

fn prompt_trigger_hints(prompts: &[PromptDef]) -> String {
    prompts
        .iter()
        .filter_map(|p| p.trigger())
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Inset the frame area to the view's territory: drops the top row
/// (safety badge) and the bottom 2 rows (action footer + overlay
/// strip). Saturates so tiny terminals don't underflow.
fn chrome_inset(area: Rect) -> Rect {
    let height = area.height.saturating_sub(3);
    Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height,
    }
}

fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars == 1 {
        return "…".into();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('…');
    out
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
    fn default_landing_view_is_dashboard() {
        let a = app();
        assert_eq!(a.active_view().id(), "dashboard");
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
        assert_eq!(a.active_view().id(), "deployments");
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
    fn describe_pane_q_and_esc_close_pane_without_quitting_app() {
        let mut a = app();
        a.describe_pane.open("test", "yaml");
        a.pane_focus = PaneFocus::Describe;
        // 'q' from inside the pane closes the pane rather than
        // quitting the whole app — matches Esc and avoids the
        // accidental-exit footgun.
        assert_eq!(a.handle_key(press(KeyCode::Char('q'))), LoopState::Continue);
        assert!(!a.describe_pane.is_open(), "q should close the pane");

        a.describe_pane.open("test", "yaml");
        a.pane_focus = PaneFocus::Describe;
        let _ = a.handle_key(press(KeyCode::Esc));
        assert!(!a.describe_pane.is_open(), "Esc should close the pane");
    }

    #[test]
    fn tab_cycles_focus_when_pane_open() {
        let mut a = app();
        a.describe_pane.open("test", "yaml");
        // Starts focused on View
        assert_eq!(a.pane_focus, PaneFocus::View);
        let _ = a.handle_key(press(KeyCode::Tab));
        assert_eq!(a.pane_focus, PaneFocus::Describe);
        let _ = a.handle_key(press(KeyCode::Tab));
        assert_eq!(a.pane_focus, PaneFocus::View);
    }

    #[test]
    fn focus_normalises_when_pane_closes() {
        let mut a = app();
        a.describe_pane.open("test", "yaml");
        a.pane_focus = PaneFocus::Describe;
        a.describe_pane.close();
        // Next handle_key normalises focus.
        let _ = a.handle_key(press(KeyCode::Char('x')));
        assert_eq!(a.pane_focus, PaneFocus::View);
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
        // Swap the default dashboard view (which loads pins from
        // ~/.config/cruster/dashboard.toml and so may have a selection
        // depending on the developer's machine) for a known-empty pods
        // view. The contract under test is "selection-requiring actions
        // are filtered out when nothing is selected", which is view-
        // agnostic.
        let mut a = app();
        a.current_view = Some(Box::new(crate::views::pods::PodsView::new()));
        let labels: Vec<&'static str> = a
            .actions
            .applicable(a.active_view())
            .map(|act| act.label())
            .collect();
        assert!(labels.contains(&"Switch kind"));
        assert!(labels.contains(&"Quit"));
        // Describe needs a selection; with an empty store, it shouldn't apply.
        assert!(!labels.contains(&"Describe"));
    }

    #[test]
    fn safety_badge_uses_env_band_fg_for_context_name() {
        use cruster_core::Environment;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut a = app();
        a.context = "my-cluster".into();
        a.environment = Environment::Unknown;
        a.read_only = false;

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let want_bg = a.theme.env_band.unknown.as_ratatui();
        let want_fg = a.theme.env_band_fg.unknown.as_ratatui();
        let mut found = false;
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            if cell.symbol() == "m" {
                assert_eq!(cell.style().bg, Some(want_bg), "ctx bg");
                assert_eq!(cell.style().fg, Some(want_fg), "ctx fg");
                found = true;
                break;
            }
        }
        assert!(found, "expected to find the 'm' of 'my-cluster' on row 0");
    }

    #[test]
    fn safety_badge_mode_chip_uses_mode_rw_color_when_writable() {
        use cruster_core::Environment;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut a = app();
        a.context = "ctx".into();
        a.environment = Environment::Unknown;
        a.read_only = false;

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let want = a.theme.mode.rw.as_ratatui();
        let mut found = false;
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            if cell.symbol() == "R" {
                assert_eq!(cell.style().fg, Some(want), "RW glyph fg");
                found = true;
                break;
            }
        }
        assert!(found, "expected to find 'R' (from [RW]) on row 0");
    }

    #[test]
    fn safety_badge_mode_chip_uses_mode_ro_color_when_read_only() {
        use cruster_core::Environment;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut a = app();
        a.context = "ctx".into();
        // Staging chosen because "STAGING" has no 'R' character, so
        // the first 'R' we find on the row must come from "[RO]".
        a.environment = Environment::Staging;
        a.read_only = true;

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let want = a.theme.mode.ro.as_ratatui();
        let mut found = false;
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            if cell.symbol() == "R" {
                assert_eq!(cell.style().fg, Some(want), "RO glyph fg");
                found = true;
                break;
            }
        }
        assert!(found, "expected to find 'R' (from [RO]) on row 0");
    }

    #[test]
    fn safety_badge_drops_layout_first_when_narrow() {
        use cruster_core::Environment;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut a = app();
        a.context = "short-ctx".into();
        a.environment = Environment::Prod;
        a.read_only = false;
        // Width chosen so env name fits but layout label does not.
        // Budget: reserved(13) + ctx(9) + sep+env(3+4) = 29.
        // Layout would add sep+layout(3+6) = 9 more -> needs width 38+.
        let backend = TestBackend::new(30, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let mut row0 = String::new();
        for x in 0..buf.area().width {
            row0.push_str(buf[(x, 0)].symbol());
        }
        assert!(row0.contains("[RW]"), "row should keep mode chip: {row0:?}");
        assert!(
            !row0.contains("single"),
            "row should not contain layout label: {row0:?}"
        );
    }

    #[test]
    fn safety_badge_truncates_context_when_extremely_narrow() {
        use cruster_core::Environment;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut a = app();
        a.context = "a-very-long-context-name-that-cannot-fit".into();
        a.environment = Environment::Unknown;
        a.read_only = false;
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let mut row0 = String::new();
        for x in 0..buf.area().width {
            row0.push_str(buf[(x, 0)].symbol());
        }
        assert!(row0.contains("[RW]"), "mode chip must survive: {row0:?}");
        assert!(row0.contains('…'), "context should be truncated: {row0:?}");
    }

    #[test]
    fn format_hint_renders_char_and_label() {
        assert_eq!(
            format_action_hint(KeyCode::Char('d'), "Describe"),
            "[d] Describe"
        );
        assert_eq!(format_action_hint(KeyCode::Esc, "Cancel"), "[esc] Cancel");
    }

    #[test]
    fn pressing_capital_p_on_free_tier_shows_pro_toast() {
        let mut a = app();
        let _ = a.handle_key(press(KeyCode::Char('P')));
        // Free tier: no armed state, no pending trigger, just a toast.
        assert!(!a.prompt_leader_armed);
        assert!(a.pending_prompt_trigger.is_none());
        assert!(a.toast.as_deref().unwrap_or("").contains("Pro"));
    }

    #[test]
    fn pressing_capital_e_on_free_tier_shows_pro_toast() {
        let mut a = app();
        let _ = a.handle_key(press(KeyCode::Char('E')));
        assert!(!a.pending_export);
        assert!(a.toast.as_deref().unwrap_or("").contains("Pro"));
    }

    #[test]
    fn armed_leader_consumes_next_char_and_defers_prompt() {
        let mut a = app();
        // Manually arm — simulates a Pro-tier user passing the gate.
        a.prompt_leader_armed = true;
        a.toast = Some("prompt: press d/w/s (or esc)".into());
        let _ = a.handle_key(press(KeyCode::Char('d')));
        assert!(!a.prompt_leader_armed);
        assert_eq!(a.pending_prompt_trigger, Some('d'));
        // The describe pane should NOT have opened — armed state
        // intercepted the `d` before keymap dispatch.
        assert!(!a.describe_pane.is_open());
    }

    #[test]
    fn armed_leader_esc_cancels_with_toast() {
        let mut a = app();
        a.prompt_leader_armed = true;
        let _ = a.handle_key(press(KeyCode::Esc));
        assert!(!a.prompt_leader_armed);
        assert!(a.pending_prompt_trigger.is_none());
        assert!(a.toast.as_deref().unwrap_or("").contains("cancelled"));
    }

    #[test]
    fn prompt_trigger_hints_lists_unique_chars() {
        let prompts = crate::prompts::load_all();
        let hints = prompt_trigger_hints(&prompts);
        // shipped defaults: d, w, s
        assert!(hints.contains('d'));
        assert!(hints.contains('w'));
        assert!(hints.contains('s'));
    }

    #[test]
    fn dashboard_is_reused_across_view_switches() {
        let mut a = app();
        assert_eq!(a.active_view().id(), "dashboard");

        // Get a pointer to the dashboard (as a raw memory address) to verify
        // it's the same instance after switching back.
        let dashboard_ptr = &a.dashboard as *const _ as usize;

        // Switch away to pods.
        a.switch_to_view_id("pods");
        assert_eq!(a.active_view().id(), "pods");

        // Switch back to dashboard.
        a.switch_to_view_id("dashboard");
        assert_eq!(a.active_view().id(), "dashboard");

        // Dashboard should be the exact same instance (sparkline history preserved).
        let dashboard_ptr_after = &a.dashboard as *const _ as usize;
        assert_eq!(
            dashboard_ptr, dashboard_ptr_after,
            "dashboard should be the same cached instance"
        );
    }

    #[test]
    fn switch_to_dashboard_succeeds() {
        let mut a = app();

        // Switch away and back to verify reload_pins path doesn't panic.
        a.switch_to_view_id("pods");
        assert_eq!(a.active_view().id(), "pods");

        a.switch_to_view_id("dashboard");
        assert_eq!(a.active_view().id(), "dashboard");
    }

    #[test]
    fn capital_d_in_read_only_mode_toasts_and_does_not_open_overlay() {
        let mut a = app();
        a.read_only = true;
        let _ = a.handle_key(press(KeyCode::Char('D')));
        assert!(a.overlay.is_none(), "no overlay should open in RO mode");
        assert!(
            a.toast.as_deref().unwrap_or("").contains("read-only"),
            "toast should explain RO gating: {:?}",
            a.toast
        );
    }

    #[test]
    fn capital_d_with_no_selection_toasts() {
        let mut a = app();
        a.read_only = false;
        // Default view is dashboard with no pins -> no selection.
        a.switch_to_view_id("pods"); // empty store -> no selection
        let _ = a.handle_key(press(KeyCode::Char('D')));
        assert!(
            a.overlay.is_none(),
            "no overlay should open without selection"
        );
        assert_eq!(a.toast.as_deref(), Some("nothing selected"));
    }

    #[test]
    fn question_mark_opens_help_overlay() {
        let mut a = app();
        let _ = a.handle_key(press(KeyCode::Char('?')));
        assert!(a.overlay.is_some(), "? should open an overlay (help)");
    }

    /// Regression: pressing `D` with the describe pane focused used
    /// to silently no-op because describe_pane.handle_key swallowed
    /// every key while open. Global semantic actions should now
    /// reach the keymap dispatch regardless of pane focus.
    #[test]
    fn capital_d_opens_delete_overlay_even_when_describe_pane_focused() {
        let mut a = app();
        a.read_only = false;
        a.switch_to_view_id("pods");
        // Force a selection so start_delete_prompt opens the overlay.
        a.current_view
            .as_mut()
            .unwrap()
            .handle_key(press(KeyCode::Char('j')));
        // Simulate the describe pane being open + focused (as it
        // would be right after the user pressed `d`).
        a.describe_pane.open("test", "yaml: contents");
        a.pane_focus = PaneFocus::Describe;
        let _ = a.handle_key(press(KeyCode::Char('D')));
        // We don't require an overlay to exist (no real selection in
        // the empty test store) — the contract is that D was NOT
        // swallowed by the describe pane. A toast OR an overlay both
        // prove dispatch reached start_delete_prompt.
        let dispatched = a.overlay.is_some()
            || a.toast
                .as_deref()
                .map(|t| t.contains("nothing selected") || t.contains("read-only"))
                .unwrap_or(false);
        assert!(
            dispatched,
            "D should dispatch globally even with describe pane focused; toast={:?}",
            a.toast,
        );
    }

    /// Navigation chords (j/k/g/G) still flow to a focused side pane
    /// so the user can scroll an open describe / logs view.
    #[test]
    fn j_still_scrolls_describe_pane_when_focused() {
        let mut a = app();
        a.describe_pane.open("test", "line1\nline2\nline3");
        a.pane_focus = PaneFocus::Describe;
        let scroll_before = a.describe_pane.scroll_for_test();
        let _ = a.handle_key(press(KeyCode::Char('j')));
        let scroll_after = a.describe_pane.scroll_for_test();
        assert_eq!(
            scroll_after,
            scroll_before + 1,
            "j should still scroll the focused describe pane",
        );
    }
}
