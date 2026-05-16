//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use cruster_kube::StoreRegistry;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;

use crate::command::{CommandAction, CommandLine};
use crate::view::ResourceView;
use crate::views::configmaps::ConfigMapsView;
use crate::views::deployments::DeploymentsView;
use crate::views::events::EventsView;
use crate::views::namespaces::NamespacesView;
use crate::views::nodes::NodesView;
use crate::views::pods::PodsView;
use crate::views::secrets::SecretsView;
use crate::views::services::ServicesView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

pub struct App {
    registry: StoreRegistry,
    current_view: Box<dyn ResourceView>,
    command: CommandLine,
    toast: Option<String>,
}

impl App {
    pub fn new(registry: StoreRegistry) -> Self {
        Self {
            registry,
            current_view: Box::new(PodsView::new()),
            command: CommandLine::new(),
            toast: None,
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

        if self.command.is_active() {
            match self.command.handle_key(key) {
                CommandAction::None | CommandAction::Cancel => {}
                CommandAction::SwitchTo(id) => match Self::view_for_id(&id) {
                    Some(v) => self.current_view = v,
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
            _ => self.current_view.handle_key(key),
        }
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
            terminal.draw(|f| {
                self.current_view.render(f);
                self.render_overlay(f);
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key) == LoopState::Quit {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn render_overlay(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if self.command.is_active() {
            let line = format!(":{}", self.command.buffer());
            let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
            let rect = Rect {
                x: area.x,
                y: area.y + area.height.saturating_sub(1),
                width: area.width,
                height: 1,
            };
            frame.render_widget(bar, rect);
        } else if let Some(msg) = &self.toast {
            let bar = Paragraph::new(msg.clone()).style(Style::default().bg(Color::Red));
            let rect = Rect {
                x: area.x,
                y: area.y + area.height.saturating_sub(1),
                width: area.width,
                height: 1,
            };
            frame.render_widget(bar, rect);
        }
    }
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
        App::new(StoreRegistry::new())
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
}
