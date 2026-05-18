//! ResourceView: trait every per-kind TUI view implements.

use async_trait::async_trait;
use crossterm::event::KeyEvent;
use cruster_core::ResourceKey;
use cruster_kube::StoreRegistry;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::LoopState;
use crate::theme::Theme;

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

    /// Render the view into `area`. The app reserves rows for chrome
    /// (safety badge, action footer, toast/overlay strip) and passes
    /// the remaining sub-rect here. Views should not call
    /// `frame.area()` directly. `theme` is threaded through so view
    /// bodies stay consistent with the user's selected theme — every
    /// foreground color decision should come from `theme` rather than
    /// a hardcoded `Color::*` literal.
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme);

    /// Handle a single key press.
    fn handle_key(&mut self, key: KeyEvent) -> LoopState;

    /// `(title, yaml)` for the currently selected resource, if any.
    /// Default implementation returns `None`; per-kind views override.
    fn selected_yaml(&self) -> Option<(String, String)> {
        None
    }

    /// The `ResourceKey` of the currently selected resource, if any.
    /// Default implementation returns `None`; per-kind views override.
    fn selected_key(&self) -> Option<ResourceKey> {
        None
    }

    /// Whether the currently selected resource can be `kubectl exec`'d
    /// into. Default `false`. PodsView overrides and returns `true`
    /// only when the pod is in the Running phase with at least one
    /// ready container — calling kubectl exec on a Completed,
    /// CrashLoopBackOff, Pending, etc. pod fails messily.
    fn selected_can_exec(&self) -> bool {
        false
    }

    /// Apply a search/filter. Default: ignored. Per-view implementations
    /// retain the filter and apply it during `refresh`.
    fn set_filter(&mut self, _filter: crate::overlays::search::Filter) {}

    /// A view may request that the app switch to a different view
    /// after `handle_key` returns — e.g. the dashboard's "open this
    /// pin in its kind view" on Enter. The app calls this once per
    /// key event and consumes the result. Default: never.
    fn take_pending_view_switch(&mut self) -> Option<&'static str> {
        None
    }

    /// A view may request a toast message after `handle_key` returns.
    /// The app calls this once per key event and consumes the result.
    /// Default: never.
    fn take_pending_toast(&mut self) -> Option<String> {
        None
    }
}
