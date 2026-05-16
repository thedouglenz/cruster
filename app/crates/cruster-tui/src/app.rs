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
use crate::prompts::PromptDef;
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
    current_view: Box<dyn ResourceView>,
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
                if let Some((title, yaml)) = self.current_view.selected_yaml() {
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
                if self.current_view.selected_key().is_none() {
                    self.toast = Some("nothing selected to export".into());
                    return LoopState::Continue;
                }
                self.pending_export = true;
                LoopState::Continue
            }
        }
    }

    fn synthetic_key_to_view(&mut self, code: KeyCode) {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        self.current_view.handle_key(key);
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

        // Make sure focus is valid (panes may have closed since last tick).
        self.normalise_pane_focus();

        // Tab cycles focus across view + open panes.
        if key.code == KeyCode::Tab {
            self.cycle_pane_focus();
            return LoopState::Continue;
        }

        // Route to focused pane (if focus is a pane and it's open).
        match self.pane_focus {
            PaneFocus::Describe if self.describe_pane.is_open() => {
                self.describe_pane.handle_key(key);
                return LoopState::Continue;
            }
            PaneFocus::Logs if self.logs_pane.is_open() => {
                self.logs_pane.handle_key(key);
                return LoopState::Continue;
            }
            _ => {}
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

        // Route through the user's keymap preset. If the chord is bound
        // to a semantic action, dispatch it. Otherwise fall through to
        // the current view (handles arrow keys etc).
        if let Some(action) = self.keymap.resolve(key.code, key.modifiers) {
            return self.dispatch_semantic(action);
        }

        self.current_view.handle_key(key)
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
            if let Some(trigger) = self.pending_prompt_trigger.take() {
                self.trigger_prompt(trigger).await;
            }
            if self.pending_export {
                self.pending_export = false;
                self.run_export().await;
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
        let selected = self.current_view.selected_key();
        if let Some(key) = &selected {
            let raw_yaml = self
                .current_view
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
                self.describe_pane
                    .render(frame, chunks[1], self.pane_focus == PaneFocus::Describe);
            } else {
                self.logs_pane
                    .render(frame, chunks[1], self.pane_focus == PaneFocus::Logs);
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
        let label = format!(
            " [{}] {} {} · layout:{} ",
            self.context,
            self.environment,
            mode,
            self.layout.label()
        );
        let color = match self.environment {
            Environment::Prod => self.theme.env_band.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band.dev.as_ratatui(),
            Environment::Local => self.theme.env_band.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band.unknown.as_ratatui(),
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
    fn describe_pane_swallows_keys_when_focused() {
        let mut a = app();
        a.describe_pane.open("test", "yaml");
        a.pane_focus = PaneFocus::Describe;
        // 'q' should not quit when describe pane is focused
        assert_eq!(a.handle_key(press(KeyCode::Char('q'))), LoopState::Continue);
        // Esc closes the pane
        let _ = a.handle_key(press(KeyCode::Esc));
        assert!(!a.describe_pane.is_open());
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
}
