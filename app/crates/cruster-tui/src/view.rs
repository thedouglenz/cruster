//! ResourceView: trait every per-kind TUI view implements.

use async_trait::async_trait;
use crossterm::event::KeyEvent;
use cruster_kube::StoreRegistry;
use ratatui::Frame;

use crate::app::LoopState;

/// One TUI view, scoped to one resource kind.
///
/// The App holds a `Box<dyn ResourceView>` for the active view and
/// calls `refresh` once per frame, then `render`, then routes keys
/// through `handle_key`. Views own their own selection state and any
/// kind-specific decoration; they read snapshots from the registry on
/// each refresh.
#[async_trait]
pub trait ResourceView: Send {
    /// Stable identifier for the view: the kind's plural ("pods",
    /// "deployments"). Used by the command-mode switcher.
    fn id(&self) -> &'static str;

    /// Refresh the view's cached snapshot from the registry. Called
    /// once per frame before `render`.
    async fn refresh(&mut self, registry: &StoreRegistry);

    /// Render the view into the full frame area.
    fn render(&self, frame: &mut Frame<'_>);

    /// Handle a single key press.
    fn handle_key(&mut self, key: KeyEvent) -> LoopState;
}
