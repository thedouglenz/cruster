//! Generic overlay machinery. Overlays are modal — when active, all
//! key events route to them until they close.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

/// What the App should do after an overlay key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayResult {
    /// Stay open; redraw on next frame.
    KeepOpen,
    /// Close the overlay. No follow-up action.
    Close,
    /// Close the overlay and invoke the named action by id.
    Invoke(String),
    /// Close the overlay and switch to the named view.
    SwitchView(String),
}

pub trait Overlay: Send {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult;
    fn render(&self, frame: &mut Frame<'_>, area: Rect);

    /// If this overlay is a live filter source (the search prompt),
    /// return its current buffer so the App can re-apply the filter
    /// on every keystroke. Default `None` for non-search overlays.
    fn live_filter_buffer(&self) -> Option<&str> {
        None
    }

    /// If this overlay is the port-forward modal, return its target
    /// pod key + current mapping buffer. Default `None`.
    fn port_forward_payload(&self) -> Option<(cruster_core::ResourceKey, String)> {
        None
    }

    /// If this overlay is the delete modal, return its target resource
    /// key + chosen propagation policy + force flag. Default `None`.
    fn delete_payload(
        &self,
    ) -> Option<(
        cruster_core::ResourceKey,
        crate::actions::delete::PropagationPolicy,
        bool,
    )> {
        None
    }
}
