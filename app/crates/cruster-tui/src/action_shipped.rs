//! Shipped actions, registered with the ActionRegistry at App startup.

use crossterm::event::KeyCode;
use cruster_core::ResourceKey;

use crate::action::Action;
use crate::view::ResourceView;

pub struct Describe;
impl Action for Describe {
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
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        view.selected_key().is_some()
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        kubectl_for_kind(view.selected_key()?, "describe")
    }
}

pub struct Logs;
impl Action for Logs {
    fn id(&self) -> &'static str {
        "logs"
    }
    fn label(&self) -> &'static str {
        "Logs"
    }
    fn description(&self) -> &'static str {
        "Stream container logs (with grep)"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('l')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        matches!(view.selected_key(), Some(k) if k.kind == "Pod")
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        if key.kind != "Pod" {
            return None;
        }
        let ns = key.namespace?;
        Some(format!("kubectl logs -f --tail=500 {} -n {}", key.name, ns))
    }
}

pub struct Exec;
impl Action for Exec {
    fn id(&self) -> &'static str {
        "exec"
    }
    fn label(&self) -> &'static str {
        "Exec into pod"
    }
    fn description(&self) -> &'static str {
        "Open a shell in the container"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('s')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        view.selected_can_exec()
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        if key.kind != "Pod" {
            return None;
        }
        let ns = key.namespace?;
        Some(format!(
            "kubectl exec -it {} -n {} -- /bin/sh",
            key.name, ns
        ))
    }
}

pub struct PortForward;
impl Action for PortForward {
    fn id(&self) -> &'static str {
        "port-forward"
    }
    fn label(&self) -> &'static str {
        "Port-forward"
    }
    fn description(&self) -> &'static str {
        "Prompt for local:remote mapping and forward"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('f')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        matches!(view.selected_key(), Some(k) if k.kind == "Pod")
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        if key.kind != "Pod" {
            return None;
        }
        let ns = key.namespace?;
        Some(format!(
            "kubectl port-forward -n {} pod/{} <LOCAL>:<REMOTE>",
            ns, key.name
        ))
    }
}

pub struct Edit;
impl Action for Edit {
    fn id(&self) -> &'static str {
        "edit"
    }
    fn label(&self) -> &'static str {
        "Edit YAML"
    }
    fn description(&self) -> &'static str {
        "Open YAML in $EDITOR and kubectl apply on save"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('e')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        matches!(view.selected_key(), Some(k) if k.kind != "Secret")
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn kubectl_equivalent(&self, view: &dyn ResourceView) -> Option<String> {
        let key = view.selected_key()?;
        let ns = key.namespace.clone();
        let kind = key.kind.to_lowercase();
        match ns {
            Some(ns) => Some(format!("kubectl edit {} {} -n {}", kind, key.name, ns)),
            None => Some(format!("kubectl edit {} {}", kind, key.name)),
        }
    }
}

pub struct SwitchKind;
impl Action for SwitchKind {
    fn id(&self) -> &'static str {
        "switch-kind"
    }
    fn label(&self) -> &'static str {
        "Switch kind"
    }
    fn description(&self) -> &'static str {
        "Open the :command-mode kind switcher"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char(':')
    }
}

pub struct Quit;
impl Action for Quit {
    fn id(&self) -> &'static str {
        "quit"
    }
    fn label(&self) -> &'static str {
        "Quit"
    }
    fn description(&self) -> &'static str {
        "Exit cruster"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('q')
    }
}

pub struct CopyKubectl;
impl Action for CopyKubectl {
    fn id(&self) -> &'static str {
        "copy-kubectl"
    }
    fn label(&self) -> &'static str {
        "Copy as kubectl"
    }
    fn description(&self) -> &'static str {
        "Copy the kubectl equivalent of the most relevant action"
    }
    fn key(&self) -> KeyCode {
        KeyCode::Char('K')
    }
    fn is_applicable(&self, view: &dyn ResourceView) -> bool {
        view.selected_key().is_some()
    }
}

fn kubectl_for_kind(key: ResourceKey, verb: &str) -> Option<String> {
    let kind = key.kind.to_lowercase();
    match key.namespace {
        Some(ns) => Some(format!("kubectl {} {} {} -n {}", verb, kind, key.name, ns)),
        None => Some(format!("kubectl {} {} {}", verb, kind, key.name)),
    }
}

/// Build the default registry with all shipped actions registered.
pub fn default_registry() -> crate::action::ActionRegistry {
    let mut r = crate::action::ActionRegistry::new();
    r.register(Box::new(Describe));
    r.register(Box::new(Logs));
    r.register(Box::new(Exec));
    r.register(Box::new(PortForward));
    r.register(Box::new(Edit));
    r.register(Box::new(SwitchKind));
    r.register(Box::new(Quit));
    r.register(Box::new(CopyKubectl));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_includes_eight_shipped_actions() {
        let r = default_registry();
        assert_eq!(r.all().len(), 8);
        for id in [
            "describe",
            "logs",
            "exec",
            "port-forward",
            "edit",
            "switch-kind",
            "quit",
            "copy-kubectl",
        ] {
            assert!(r.by_id(id).is_some(), "missing action: {id}");
        }
    }

    #[test]
    fn logs_kubectl_for_pod() {
        let action = Logs;
        let key = ResourceKey::namespaced("Pod", "default", "nginx");
        struct V(ResourceKey);
        #[async_trait::async_trait]
        impl ResourceView for V {
            fn id(&self) -> &'static str {
                "test"
            }
            async fn refresh(&mut self, _r: &cruster_kube::StoreRegistry) {}
            fn render(&self, _f: &mut ratatui::Frame<'_>, _area: ratatui::layout::Rect) {}
            fn handle_key(&mut self, _k: crossterm::event::KeyEvent) -> crate::app::LoopState {
                crate::app::LoopState::Continue
            }
            fn selected_key(&self) -> Option<ResourceKey> {
                Some(self.0.clone())
            }
        }
        assert_eq!(
            action.kubectl_equivalent(&V(key)),
            Some("kubectl logs -f --tail=500 nginx -n default".into())
        );
    }

    #[test]
    fn edit_is_destructive() {
        assert!(Edit.is_destructive());
        assert!(!Describe.is_destructive());
    }
}
