//! Application state and event loop.

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use cruster_kube::ResourceStore;
use k8s_openapi::api::core::v1::Pod;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::views::pods::PodsView;

/// Whether the app should keep running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopState {
    Continue,
    Quit,
}

pub struct App {
    pod_store: ResourceStore<Pod>,
    pods_view: PodsView,
}

impl App {
    pub fn new(pod_store: ResourceStore<Pod>) -> Self {
        Self {
            pod_store,
            pods_view: PodsView::new(),
        }
    }

    /// Pure handler — no I/O. Returns what the loop should do next.
    ///
    /// `row_count` is the current visible row count; we need it to
    /// clamp `move_down` past the end.
    pub fn handle_key(&mut self, key: KeyEvent, row_count: usize) -> LoopState {
        if key.kind != KeyEventKind::Press {
            return LoopState::Continue;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => LoopState::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.pods_view.move_down(row_count);
                LoopState::Continue
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.pods_view.move_up();
                LoopState::Continue
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.pods_view.move_to_top();
                LoopState::Continue
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.pods_view.move_to_bottom(row_count);
                LoopState::Continue
            }
            _ => LoopState::Continue,
        }
    }

    /// Main loop. Runs until the user quits or the terminal closes.
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
            let snapshot = self.pod_store.snapshot().await;
            let row_count = snapshot.len();
            terminal.draw(|f| self.pods_view.render(f, &snapshot))?;

            // Poll with a short timeout so the snapshot refreshes when
            // no key is pressed.
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key, row_count) == LoopState::Quit {
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
        App::new(ResourceStore::<Pod>::new())
    }

    #[test]
    fn q_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Char('q')), 0), LoopState::Quit);
    }

    #[test]
    fn esc_quits() {
        let mut a = app();
        assert_eq!(a.handle_key(press(KeyCode::Esc), 0), LoopState::Quit);
    }

    #[test]
    fn unknown_key_continues() {
        let mut a = app();
        assert_eq!(
            a.handle_key(press(KeyCode::Char('x')), 0),
            LoopState::Continue
        );
    }

    #[test]
    fn down_advances_selection() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char('j')), 3);
        assert_eq!(a.pods_view.selected(), 1);
    }

    #[test]
    fn down_clamps_at_last_row() {
        let mut a = app();
        for _ in 0..10 {
            a.handle_key(press(KeyCode::Char('j')), 3);
        }
        assert_eq!(a.pods_view.selected(), 2);
    }

    #[test]
    fn up_does_not_underflow() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char('k')), 3);
        assert_eq!(a.pods_view.selected(), 0);
    }

    #[test]
    fn capital_g_jumps_to_bottom() {
        let mut a = app();
        a.handle_key(press(KeyCode::Char('G')), 5);
        assert_eq!(a.pods_view.selected(), 4);
    }

    #[test]
    fn key_release_is_ignored() {
        let mut a = app();
        let release = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(a.handle_key(release, 0), LoopState::Continue);
    }
}
