//! Exec action: suspends the TUI and runs `kubectl exec -it`.
//!
//! We shell out to kubectl rather than implementing the attach
//! protocol natively. kubectl already nails the TTY plumbing; doing
//! it ourselves with kube-rs is a substantial side-quest we don't
//! need in v1.

use std::io;
use std::process::Command;

use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use cruster_core::ResourceKey;

/// Run `kubectl exec -it <pod> -n <ns> -- $SHELL` in the current
/// terminal, restoring the TUI when it exits.
///
/// Returns an error if the resource is cluster-scoped (exec doesn't
/// apply), if kubectl is not on `$PATH`, or if the child exits non-zero.
/// Callers should display the error message as a toast rather than
/// propagating — exec exited non-zero is normal (e.g. user pressed
/// Ctrl-D in the shell).
pub fn exec_into(key: &ResourceKey) -> anyhow::Result<()> {
    let ns = key
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("exec requires a namespaced resource"))?;
    if key.kind != "Pod" {
        anyhow::bail!("exec is only supported for pods (selected: {})", key.kind);
    }

    // Suspend the TUI.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let status = Command::new("kubectl")
        .args(["exec", "-it", "-n", ns, &key.name, "--", &shell])
        .status();

    // Restore the TUI before doing anything else.
    let _ = enable_raw_mode();
    let _ = execute!(io::stdout(), EnterAlternateScreen);

    let status = status?;
    if !status.success() {
        anyhow::bail!("kubectl exec exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_into_rejects_cluster_scoped() {
        let key = ResourceKey::cluster_scoped("Node", "n1");
        let r = exec_into(&key);
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("namespaced"));
    }

    #[test]
    fn exec_into_rejects_non_pod_kinds() {
        let key = ResourceKey::namespaced("Deployment", "default", "web");
        let r = exec_into(&key);
        assert!(r.is_err());
        assert!(r
            .unwrap_err()
            .to_string()
            .contains("only supported for pods"));
    }
}
