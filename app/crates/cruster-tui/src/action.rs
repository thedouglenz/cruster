//! Action registry: metadata for every action a user can take in the
//! TUI. Used by the command palette, the inline action footer, and
//! the "copy as kubectl" feature.

use crossterm::event::KeyCode;

use crate::view::ResourceView;

/// Single TUI action: a thing the user can do.
pub trait Action: Send + Sync {
    /// Stable identifier (`describe`, `logs`, `exec`, …).
    fn id(&self) -> &'static str;

    /// Human label for palettes/footer.
    fn label(&self) -> &'static str;

    /// One-line description.
    fn description(&self) -> &'static str;

    /// Primary keybinding.
    fn key(&self) -> KeyCode;

    /// Whether this action makes sense for the current view's
    /// selection. Default: always applicable.
    fn is_applicable(&self, _view: &dyn ResourceView) -> bool {
        true
    }

    /// Whether this action modifies cluster state.
    fn is_destructive(&self) -> bool {
        false
    }

    /// The equivalent kubectl command, if expressible. Used by
    /// the "copy as kubectl" feature.
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let _ = view;
        None
    }
}

/// Container of all known actions. Built once at App startup.
#[derive(Default)]
pub struct ActionRegistry {
    actions: Vec<Box<dyn Action>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, action: Box<dyn Action>) {
        self.actions.push(action);
    }

    pub fn all(&self) -> &[Box<dyn Action>] {
        &self.actions
    }

    /// Actions applicable to the current view's selection. Used by
    /// the inline action footer.
    pub fn applicable<'a>(
        &'a self,
        view: &'a dyn ResourceView,
    ) -> impl Iterator<Item = &'a dyn Action> + 'a {
        self.actions
            .iter()
            .map(|a| a.as_ref())
            .filter(|a| a.is_applicable(view))
    }

    /// Look up an action by id.
    pub fn by_id(&self, id: &str) -> Option<&dyn Action> {
        self.actions
            .iter()
            .find(|a| a.id() == id)
            .map(|a| a.as_ref())
    }

    /// Look up an action by key. Returns the first match.
    pub fn by_key(&self, key: KeyCode) -> Option<&dyn Action> {
        self.actions
            .iter()
            .find(|a| a.key() == key)
            .map(|a| a.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cruster_core::ResourceKey;
    use cruster_kube::StoreRegistry;
    use ratatui::Frame;

    struct DummyView;

    #[async_trait]
    impl ResourceView for DummyView {
        fn id(&self) -> &'static str {
            "dummy"
        }
        async fn refresh(&mut self, _registry: &StoreRegistry) {}
        fn render(&self, _frame: &mut Frame<'_>, _area: ratatui::layout::Rect) {}
        fn handle_key(&mut self, _key: crossterm::event::KeyEvent) -> crate::app::LoopState {
            crate::app::LoopState::Continue
        }
        fn selected_key(&self) -> Option<ResourceKey> {
            Some(ResourceKey::namespaced("Pod", "default", "x"))
        }
    }

    struct DescribeAction;
    impl Action for DescribeAction {
        fn id(&self) -> &'static str {
            "describe"
        }
        fn label(&self) -> &'static str {
            "Describe"
        }
        fn description(&self) -> &'static str {
            "Open YAML pane on the selected resource"
        }
        fn key(&self) -> KeyCode {
            KeyCode::Char('d')
        }
        fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
            let key = view.selected_key()?;
            let ns = key.namespace?;
            Some(format!(
                "kubectl describe {} {} -n {}",
                key.kind.to_lowercase(),
                key.name,
                ns
            ))
        }
    }

    #[test]
    fn registry_round_trip() {
        let mut r = ActionRegistry::new();
        r.register(Box::new(DescribeAction));
        assert!(r.by_id("describe").is_some());
        assert!(r.by_id("nope").is_none());
        assert!(r.by_key(KeyCode::Char('d')).is_some());
        assert!(r.by_key(KeyCode::Char('z')).is_none());
    }

    #[test]
    fn applicable_returns_all_by_default() {
        let mut r = ActionRegistry::new();
        r.register(Box::new(DescribeAction));
        let view = DummyView;
        assert_eq!(r.applicable(&view).count(), 1);
    }

    #[test]
    fn kubectl_equivalent_formats_with_selection() {
        let action = DescribeAction;
        let view = DummyView;
        assert_eq!(
            action.kubectl_equivalent(&view),
            Some("kubectl describe pod x -n default".into())
        );
    }
}
