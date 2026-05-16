//! Environment classification and read-only mode.

use std::path::PathBuf;

use cruster_core::Environment;
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SafetyConfig {
    #[serde(default)]
    pub matchers: Matchers,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Matchers {
    #[serde(default)]
    pub prod: Vec<String>,
    #[serde(default)]
    pub staging: Vec<String>,
    #[serde(default)]
    pub dev: Vec<String>,
    #[serde(default)]
    pub local: Vec<String>,
}

impl SafetyConfig {
    pub fn defaults() -> Self {
        Self {
            matchers: Matchers {
                prod: vec!["^prod-".into(), "production".into()],
                staging: vec!["^staging-".into(), "stg-".into()],
                dev: vec!["^dev-".into()],
                local: vec!["^k3d-".into(), "^kind-".into(), "^minikube".into()],
            },
        }
    }

    /// Load from `~/.config/cruster/safety.toml`, falling back to
    /// defaults on any read or parse error.
    pub fn load_or_default() -> Self {
        let Some(path) = config_path() else {
            return Self::defaults();
        };
        let Ok(body) = std::fs::read_to_string(&path) else {
            return Self::defaults();
        };
        toml::from_str(&body).unwrap_or_else(|_| Self::defaults())
    }

    pub fn classify(&self, context: &str) -> Environment {
        if any_match(&self.matchers.prod, context) {
            Environment::Prod
        } else if any_match(&self.matchers.staging, context) {
            Environment::Staging
        } else if any_match(&self.matchers.dev, context) {
            Environment::Dev
        } else if any_match(&self.matchers.local, context) {
            Environment::Local
        } else {
            Environment::Unknown
        }
    }
}

fn any_match(patterns: &[String], s: &str) -> bool {
    patterns.iter().any(|p| {
        if let Some(prefix) = p.strip_prefix('^') {
            s.starts_with(prefix)
        } else {
            s.contains(p.as_str())
        }
    })
}

fn config_path() -> Option<PathBuf> {
    let mut p = dirs::config_dir()?;
    p.push("cruster");
    p.push("safety.toml");
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_classify_known_prefixes() {
        let cfg = SafetyConfig::defaults();
        assert_eq!(cfg.classify("prod-us-east"), Environment::Prod);
        assert_eq!(cfg.classify("production"), Environment::Prod);
        assert_eq!(cfg.classify("staging-eu"), Environment::Staging);
        assert_eq!(cfg.classify("dev-1"), Environment::Dev);
        assert_eq!(cfg.classify("k3d-a8s-dev"), Environment::Local);
        assert_eq!(cfg.classify("kind-foo"), Environment::Local);
        assert_eq!(cfg.classify("minikube"), Environment::Local);
        assert_eq!(cfg.classify("random-cluster"), Environment::Unknown);
    }

    #[test]
    fn matchers_can_be_overridden_by_user() {
        let body = r#"
            [matchers]
            prod = ["my-special-prod"]
        "#;
        let cfg: SafetyConfig = toml::from_str(body).unwrap();
        assert_eq!(cfg.classify("my-special-prod-1"), Environment::Prod);
        assert_eq!(cfg.classify("prod-us-east"), Environment::Unknown);
    }
}
