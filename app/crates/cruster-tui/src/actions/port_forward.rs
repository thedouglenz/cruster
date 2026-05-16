//! Port-forward action: spawns `kubectl port-forward` as a background
//! child process and tracks it for later cancellation.

use std::process::{Child, Command, Stdio};

use cruster_core::ResourceKey;

#[derive(Debug)]
pub struct PortForward {
    pub key: ResourceKey,
    pub mapping: String, // "8080:80"
    child: Child,
}

impl PortForward {
    pub fn start(key: ResourceKey, mapping: String) -> anyhow::Result<Self> {
        let ns = key
            .namespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("port-forward requires a namespaced resource"))?;
        if key.kind != "Pod" {
            anyhow::bail!(
                "port-forward is only supported for pods (selected: {})",
                key.kind
            );
        }
        if !is_valid_mapping(&mapping) {
            anyhow::bail!("invalid mapping '{mapping}' — expected `local:remote` (e.g. 8080:80)");
        }
        let child = Command::new("kubectl")
            .args([
                "port-forward",
                "-n",
                ns,
                &format!("pod/{}", key.name),
                &mapping,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            key,
            mapping,
            child,
        })
    }

    fn cancel(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PortForward {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Validates "local:remote" mapping — both parts must parse as u16.
fn is_valid_mapping(mapping: &str) -> bool {
    let mut parts = mapping.split(':');
    let (Some(l), Some(r), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    l.parse::<u16>().is_ok() && r.parse::<u16>().is_ok()
}

#[derive(Default)]
pub struct PortForwards {
    active: Vec<PortForward>,
}

impl PortForwards {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, pf: PortForward) {
        self.active.push(pf);
    }

    pub fn len(&self) -> usize {
        self.active.len()
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_scoped_rejected() {
        let key = ResourceKey::cluster_scoped("Node", "n1");
        let r = PortForward::start(key, "8080:80".into());
        assert!(r.is_err());
    }

    #[test]
    fn non_pod_kind_rejected() {
        let key = ResourceKey::namespaced("Deployment", "default", "web");
        let r = PortForward::start(key, "8080:80".into());
        assert!(r.is_err());
        assert!(r
            .unwrap_err()
            .to_string()
            .contains("only supported for pods"));
    }

    #[test]
    fn invalid_mapping_rejected() {
        let key = ResourceKey::namespaced("Pod", "default", "nginx");
        let r = PortForward::start(key, "not-a-mapping".into());
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("invalid mapping"));
    }

    #[test]
    fn mapping_validation_accepts_two_u16() {
        assert!(is_valid_mapping("8080:80"));
        assert!(is_valid_mapping("0:0"));
        assert!(is_valid_mapping("65535:65535"));
    }

    #[test]
    fn mapping_validation_rejects_garbage() {
        assert!(!is_valid_mapping(""));
        assert!(!is_valid_mapping("8080"));
        assert!(!is_valid_mapping("8080:"));
        assert!(!is_valid_mapping(":80"));
        assert!(!is_valid_mapping("a:80"));
        assert!(!is_valid_mapping("8080:80:90"));
        assert!(!is_valid_mapping("65536:80")); // > u16::MAX
    }
}
