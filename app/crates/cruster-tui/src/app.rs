//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use cruster_kube::StoreRegistry;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::view::ResourceView;
use crate::views::pods::PodsView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

pub struct App {
    registry: StoreRegistry,
    current_view: Box<dyn ResourceView>,
}

impl App {
    pub fn new(registry: StoreRegistry) -> Self {
        Self {
            registry,
            current_view: Box::new(PodsView::new()),
        }
    }

    /// Replace the currently active view.
    pub fn switch_view(&mut self, view: Box<dyn ResourceView>) {
        self.current_view = view;
    }

    /// Pure, top-level key handler.
    ///
    /// Returns `LoopState::Quit` for global quit (`q`, `Esc`).
    /// Otherwise delegates to the current view.
    pub fn handle_key(&mut self, key: KeyEvent) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
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
            terminal.draw(|f| self.current_view.render(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key) == LoopState::Quit {
                        return Ok(());
                    }
                }
            }
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
}
