//! Delete action: shells out to `kubectl delete` for the selected
//! resource with the user's chosen propagation policy + optional
//! force/grace-period override.

use std::process::Command;

use cruster_core::ResourceKey;

/// Cascade strategy for the apiserver's garbage collector.
/// Maps to `kubectl delete --cascade=<value>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropagationPolicy {
    /// GC deletes dependents in the background after the parent is
    /// gone. This is `kubectl`'s default.
    Background,
    /// Parent waits until dependents are gone before being deleted.
    Foreground,
    /// Parent is deleted; dependents are not garbage collected.
    Orphan,
}

impl PropagationPolicy {
    pub fn flag_value(self) -> &'static str {
        match self {
            PropagationPolicy::Background => "background",
            PropagationPolicy::Foreground => "foreground",
            PropagationPolicy::Orphan => "orphan",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PropagationPolicy::Background => "Background",
            PropagationPolicy::Foreground => "Foreground",
            PropagationPolicy::Orphan => "Orphan",
        }
    }
}

/// Build the argv for the `kubectl delete` invocation. Split from the
/// spawn call so the formatting can be unit-tested without touching
/// the process table.
pub fn kubectl_argv(key: &ResourceKey, policy: PropagationPolicy, force: bool) -> Vec<String> {
    let kind = key.kind.to_lowercase();
    let mut args: Vec<String> = vec!["delete".into(), kind, key.name.clone()];
    if let Some(ns) = key.namespace.as_deref() {
        args.push("-n".into());
        args.push(ns.into());
    }
    args.push(format!("--cascade={}", policy.flag_value()));
    if force {
        args.push("--grace-period=0".into());
        args.push("--force".into());
    }
    args
}

/// Same argv but rendered as the literal command-line a user could
/// paste into a shell. Used by the `K` (copy as kubectl) action.
pub fn kubectl_command(key: &ResourceKey, policy: PropagationPolicy, force: bool) -> String {
    let mut s = String::from("kubectl ");
    s.push_str(&kubectl_argv(key, policy, force).join(" "));
    s
}

/// Run `kubectl delete ...` synchronously and return Ok(()) on success
/// or an error containing the captured stderr's first line. The TUI
/// schedules this via `tokio::task::spawn_blocking` so the event loop
/// keeps drawing.
pub fn run_delete(key: &ResourceKey, policy: PropagationPolicy, force: bool) -> anyhow::Result<()> {
    let args = kubectl_argv(key, policy, force);
    let output = Command::new("kubectl").args(&args).output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first = stderr
        .lines()
        .next()
        .unwrap_or("kubectl delete failed")
        .trim();
    anyhow::bail!("{}", first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_namespaced_default_is_background() {
        let key = ResourceKey::namespaced("Pod", "default", "nginx");
        let argv = kubectl_argv(&key, PropagationPolicy::Background, false);
        assert_eq!(
            argv,
            vec![
                "delete",
                "pod",
                "nginx",
                "-n",
                "default",
                "--cascade=background"
            ]
        );
    }

    #[test]
    fn argv_cluster_scoped_omits_namespace_flag() {
        let key = ResourceKey::cluster_scoped("Namespace", "foo");
        let argv = kubectl_argv(&key, PropagationPolicy::Foreground, false);
        assert_eq!(
            argv,
            vec!["delete", "namespace", "foo", "--cascade=foreground"]
        );
    }

    #[test]
    fn argv_with_force_appends_grace_period_zero_and_force() {
        let key = ResourceKey::namespaced("Pod", "default", "stuck");
        let argv = kubectl_argv(&key, PropagationPolicy::Orphan, true);
        assert_eq!(
            argv,
            vec![
                "delete",
                "pod",
                "stuck",
                "-n",
                "default",
                "--cascade=orphan",
                "--grace-period=0",
                "--force",
            ]
        );
    }

    #[test]
    fn kubectl_command_string_is_pasteable() {
        let key = ResourceKey::namespaced("Deployment", "web", "api");
        assert_eq!(
            kubectl_command(&key, PropagationPolicy::Background, false),
            "kubectl delete deployment api -n web --cascade=background"
        );
        assert_eq!(
            kubectl_command(&key, PropagationPolicy::Foreground, true),
            "kubectl delete deployment api -n web --cascade=foreground --grace-period=0 --force"
        );
    }

    #[test]
    fn policy_flag_values_match_kubectl_grammar() {
        assert_eq!(PropagationPolicy::Background.flag_value(), "background");
        assert_eq!(PropagationPolicy::Foreground.flag_value(), "foreground");
        assert_eq!(PropagationPolicy::Orphan.flag_value(), "orphan");
    }

    #[test]
    fn policy_labels_are_titlecase() {
        assert_eq!(PropagationPolicy::Background.label(), "Background");
        assert_eq!(PropagationPolicy::Foreground.label(), "Foreground");
        assert_eq!(PropagationPolicy::Orphan.label(), "Orphan");
    }
}
