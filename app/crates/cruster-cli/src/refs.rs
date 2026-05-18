//! Pure helpers for walking the reference graph of a Pod / PodSpec.
//!
//! Currently surfaces just the config refs (Secrets, ConfigMaps reached
//! via env / envFrom / volumes / projected sources) that the `timeline`
//! and `changed` verbs need to know about. Kept separate from any
//! particular verb so both can share without circular imports.

use std::collections::BTreeSet;

use k8s_openapi::api::core::v1::{Container, PodSpec, Volume};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigKind {
    Secret,
    ConfigMap,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConfigRef {
    pub kind: ConfigKind,
    pub name: String,
    /// `env` | `envFrom` | `volume` | `projected`.
    pub via: &'static str,
    /// Container name where the reference appears; `None` for
    /// volume-scoped references (volumes are pod-level).
    pub container: Option<String>,
}

/// Walk a PodSpec and return every Secret / ConfigMap it references,
/// deduplicated by (kind, name, via, container).
pub fn collect_config_refs(spec: &PodSpec) -> Vec<ConfigRef> {
    let mut out: BTreeSet<ConfigRef> = BTreeSet::new();
    for c in &spec.containers {
        collect_from_container(c, &mut out);
    }
    if let Some(inits) = &spec.init_containers {
        for c in inits {
            collect_from_container(c, &mut out);
        }
    }
    for v in spec.volumes.as_deref().unwrap_or(&[]) {
        collect_from_volume(v, &mut out);
    }
    out.into_iter().collect()
}

fn collect_from_container(c: &Container, out: &mut BTreeSet<ConfigRef>) {
    if let Some(envs) = &c.env {
        for e in envs {
            if let Some(vf) = &e.value_from {
                if let Some(sref) = &vf.secret_key_ref {
                    out.insert(ConfigRef {
                        kind: ConfigKind::Secret,
                        name: sref.name.clone(),
                        via: "env",
                        container: Some(c.name.clone()),
                    });
                }
                if let Some(cref) = &vf.config_map_key_ref {
                    out.insert(ConfigRef {
                        kind: ConfigKind::ConfigMap,
                        name: cref.name.clone(),
                        via: "env",
                        container: Some(c.name.clone()),
                    });
                }
            }
        }
    }
    if let Some(efs) = &c.env_from {
        for e in efs {
            if let Some(sref) = &e.secret_ref {
                out.insert(ConfigRef {
                    kind: ConfigKind::Secret,
                    name: sref.name.clone(),
                    via: "envFrom",
                    container: Some(c.name.clone()),
                });
            }
            if let Some(cref) = &e.config_map_ref {
                out.insert(ConfigRef {
                    kind: ConfigKind::ConfigMap,
                    name: cref.name.clone(),
                    via: "envFrom",
                    container: Some(c.name.clone()),
                });
            }
        }
    }
}

fn collect_from_volume(v: &Volume, out: &mut BTreeSet<ConfigRef>) {
    if let Some(s) = &v.secret {
        if let Some(name) = &s.secret_name {
            out.insert(ConfigRef {
                kind: ConfigKind::Secret,
                name: name.clone(),
                via: "volume",
                container: None,
            });
        }
    }
    if let Some(cm) = &v.config_map {
        out.insert(ConfigRef {
            kind: ConfigKind::ConfigMap,
            name: cm.name.clone(),
            via: "volume",
            container: None,
        });
    }
    if let Some(proj) = &v.projected {
        if let Some(sources) = &proj.sources {
            for src in sources {
                if let Some(s) = &src.secret {
                    out.insert(ConfigRef {
                        kind: ConfigKind::Secret,
                        name: s.name.clone(),
                        via: "projected",
                        container: None,
                    });
                }
                if let Some(cm) = &src.config_map {
                    out.insert(ConfigRef {
                        kind: ConfigKind::ConfigMap,
                        name: cm.name.clone(),
                        via: "projected",
                        container: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ConfigMapKeySelector, ConfigMapVolumeSource, EnvFromSource, EnvVar, EnvVarSource,
        SecretEnvSource, SecretKeySelector, SecretVolumeSource,
    };

    fn pod_with_refs() -> PodSpec {
        let c = Container {
            name: "app".into(),
            env: Some(vec![
                EnvVar {
                    name: "TOKEN".into(),
                    value_from: Some(EnvVarSource {
                        secret_key_ref: Some(SecretKeySelector {
                            name: "api-creds".into(),
                            key: "token".into(),
                            optional: None,
                        }),
                        ..Default::default()
                    }),
                    value: None,
                },
                EnvVar {
                    name: "FEATURE_FLAG".into(),
                    value_from: Some(EnvVarSource {
                        config_map_key_ref: Some(ConfigMapKeySelector {
                            name: "features".into(),
                            key: "enabled".into(),
                            optional: None,
                        }),
                        ..Default::default()
                    }),
                    value: None,
                },
            ]),
            env_from: Some(vec![EnvFromSource {
                prefix: None,
                secret_ref: Some(SecretEnvSource {
                    name: "bulk-secret".into(),
                    optional: None,
                }),
                config_map_ref: None,
            }]),
            ..Default::default()
        };
        PodSpec {
            containers: vec![c],
            volumes: Some(vec![Volume {
                name: "cfg".into(),
                config_map: Some(ConfigMapVolumeSource {
                    name: "file-cfg".into(),
                    ..Default::default()
                }),
                secret: Some(SecretVolumeSource {
                    secret_name: Some("file-sec".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn collect_config_refs_walks_env_envfrom_and_volumes() {
        let spec = pod_with_refs();
        let refs = collect_config_refs(&spec);
        let names: Vec<(&str, &str)> = refs
            .iter()
            .map(|r| {
                (
                    match r.kind {
                        ConfigKind::Secret => "Secret",
                        ConfigKind::ConfigMap => "ConfigMap",
                    },
                    r.name.as_str(),
                )
            })
            .collect();
        assert!(names.contains(&("Secret", "api-creds")));
        assert!(names.contains(&("ConfigMap", "features")));
        assert!(names.contains(&("Secret", "bulk-secret")));
        assert!(names.contains(&("ConfigMap", "file-cfg")));
        assert!(names.contains(&("Secret", "file-sec")));
        assert_eq!(refs.len(), 5);
    }

    #[test]
    fn collect_config_refs_dedupes_identical_references() {
        // Two containers referencing the same secret via envFrom — one
        // ConfigRef per (kind, name, via, container), so the two
        // container-scoped references stay distinct (good signal for
        // the LLM about which container consumes it).
        let mk_container = |name: &str| Container {
            name: name.into(),
            env_from: Some(vec![EnvFromSource {
                prefix: None,
                secret_ref: Some(SecretEnvSource {
                    name: "shared".into(),
                    optional: None,
                }),
                config_map_ref: None,
            }]),
            ..Default::default()
        };
        let spec = PodSpec {
            containers: vec![mk_container("a"), mk_container("b")],
            ..Default::default()
        };
        let refs = collect_config_refs(&spec);
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().all(|r| r.name == "shared"));
        assert!(refs.iter().all(|r| r.via == "envFrom"));
    }

    #[test]
    fn init_containers_are_walked_too() {
        let spec = PodSpec {
            init_containers: Some(vec![Container {
                name: "init".into(),
                env: Some(vec![EnvVar {
                    name: "MIGRATE_KEY".into(),
                    value_from: Some(EnvVarSource {
                        secret_key_ref: Some(SecretKeySelector {
                            name: "migrate".into(),
                            key: "key".into(),
                            optional: None,
                        }),
                        ..Default::default()
                    }),
                    value: None,
                }]),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let refs = collect_config_refs(&spec);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "migrate");
        assert_eq!(refs[0].container.as_deref(), Some("init"));
    }
}
