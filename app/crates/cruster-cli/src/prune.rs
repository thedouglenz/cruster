//! Field pruning: remove apiserver noise from serialised k8s objects.
//!
//! Default (LLM/non-full mode) drops:
//! - `metadata.managedFields`
//! - `metadata.annotations["kubectl.kubernetes.io/last-applied-configuration"]`
//! - `metadata.resourceVersion`, `metadata.uid`, `metadata.generation`
//! - `metadata.creationTimestamp`
//! - `status.conditions[*].lastTransitionTime`
//! - `status.conditions[*].lastHeartbeatTime`

use serde_json::Value;

/// Prune in-place. No-op if `full` is true.
pub fn prune(value: &mut Value, full: bool) {
    if full {
        return;
    }
    prune_metadata(value);
    prune_status_conditions(value);
}

fn prune_metadata(value: &mut Value) {
    let Some(meta) = value.get_mut("metadata").and_then(|m| m.as_object_mut()) else {
        return;
    };
    meta.remove("managedFields");
    meta.remove("resourceVersion");
    meta.remove("uid");
    meta.remove("generation");
    meta.remove("creationTimestamp");
    if let Some(annotations) = meta.get_mut("annotations").and_then(|a| a.as_object_mut()) {
        annotations.remove("kubectl.kubernetes.io/last-applied-configuration");
        if annotations.is_empty() {
            meta.remove("annotations");
        }
    }
}

fn prune_status_conditions(value: &mut Value) {
    let Some(conditions) = value
        .get_mut("status")
        .and_then(|s| s.get_mut("conditions"))
        .and_then(|c| c.as_array_mut())
    else {
        return;
    };
    for c in conditions {
        if let Some(obj) = c.as_object_mut() {
            obj.remove("lastTransitionTime");
            obj.remove("lastHeartbeatTime");
        }
    }
}

/// Redact a Secret's `data` and `stringData` values. Always applied
/// regardless of `--full` — secrets are never emitted in cleartext.
pub fn redact_secret(value: &mut Value) {
    if let Some(data) = value.get_mut("data").and_then(|d| d.as_object_mut()) {
        for (_, v) in data.iter_mut() {
            *v = Value::String("<redacted>".into());
        }
    }
    if let Some(string_data) = value.get_mut("stringData").and_then(|d| d.as_object_mut()) {
        for (_, v) in string_data.iter_mut() {
            *v = Value::String("<redacted>".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_mode_is_noop() {
        let mut v = json!({"metadata": {"managedFields": [1, 2, 3]}});
        prune(&mut v, true);
        assert!(v["metadata"]["managedFields"].is_array());
    }

    #[test]
    fn managed_fields_removed_by_default() {
        let mut v = json!({"metadata": {"name": "x", "managedFields": [1, 2, 3]}});
        prune(&mut v, false);
        assert!(v["metadata"].get("managedFields").is_none());
        assert_eq!(v["metadata"]["name"], "x");
    }

    #[test]
    fn last_applied_annotation_removed() {
        let mut v = json!({
            "metadata": {
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{...big blob...}",
                    "app.kubernetes.io/name": "nginx"
                }
            }
        });
        prune(&mut v, false);
        let ann = &v["metadata"]["annotations"];
        assert!(ann
            .get("kubectl.kubernetes.io/last-applied-configuration")
            .is_none());
        assert_eq!(ann["app.kubernetes.io/name"], "nginx");
    }

    #[test]
    fn empty_annotations_block_removed_entirely() {
        let mut v = json!({
            "metadata": {
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{...}"
                }
            }
        });
        prune(&mut v, false);
        assert!(v["metadata"].get("annotations").is_none());
    }

    #[test]
    fn status_condition_timestamps_removed() {
        let mut v = json!({
            "status": {
                "conditions": [
                    {
                        "type": "Ready",
                        "status": "True",
                        "lastTransitionTime": "2026-01-01T00:00:00Z",
                        "lastHeartbeatTime": "2026-01-01T00:01:00Z"
                    }
                ]
            }
        });
        prune(&mut v, false);
        let cond = &v["status"]["conditions"][0];
        assert!(cond.get("lastTransitionTime").is_none());
        assert!(cond.get("lastHeartbeatTime").is_none());
        assert_eq!(cond["type"], "Ready");
        assert_eq!(cond["status"], "True");
    }

    #[test]
    fn missing_metadata_is_noop() {
        let mut v = json!({"foo": "bar"});
        prune(&mut v, false);
        assert_eq!(v["foo"], "bar");
    }

    #[test]
    fn redact_secret_replaces_data_values() {
        let mut v = json!({
            "data": {
                "password": "U1VQRVJfU0VDUkVU",
                "token": "dG9rX2FiYw=="
            },
            "stringData": {
                "username": "admin"
            }
        });
        redact_secret(&mut v);
        assert_eq!(v["data"]["password"], "<redacted>");
        assert_eq!(v["data"]["token"], "<redacted>");
        assert_eq!(v["stringData"]["username"], "<redacted>");
    }

    #[test]
    fn redact_secret_keeps_other_fields() {
        let mut v = json!({
            "metadata": {"name": "x"},
            "type": "Opaque",
            "data": {"k": "v"}
        });
        redact_secret(&mut v);
        assert_eq!(v["metadata"]["name"], "x");
        assert_eq!(v["type"], "Opaque");
        assert_eq!(v["data"]["k"], "<redacted>");
    }
}
