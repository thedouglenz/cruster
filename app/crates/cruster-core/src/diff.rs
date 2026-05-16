//! Structural diff over `serde_json::Value`.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Change {
    Added { path: String, value: Value },
    Removed { path: String, value: Value },
    Modified { path: String, old: Value, new: Value },
}

pub fn diff(a: &Value, b: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    walk("", a, b, &mut out);
    out
}

fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<Change>) {
    match (a, b) {
        (Value::Object(oa), Value::Object(ob)) => {
            let mut keys: std::collections::BTreeSet<&String> = oa.keys().collect();
            keys.extend(ob.keys());
            for k in keys {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match (oa.get(k), ob.get(k)) {
                    (Some(va), Some(vb)) => walk(&p, va, vb, out),
                    (None, Some(vb)) => out.push(Change::Added {
                        path: p,
                        value: vb.clone(),
                    }),
                    (Some(va), None) => out.push(Change::Removed {
                        path: p,
                        value: va.clone(),
                    }),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(aa), Value::Array(ab)) => {
            let len = aa.len().max(ab.len());
            for i in 0..len {
                let p = format!("{path}[{i}]");
                match (aa.get(i), ab.get(i)) {
                    (Some(va), Some(vb)) => walk(&p, va, vb, out),
                    (None, Some(vb)) => out.push(Change::Added {
                        path: p,
                        value: vb.clone(),
                    }),
                    (Some(va), None) => out.push(Change::Removed {
                        path: p,
                        value: va.clone(),
                    }),
                    (None, None) => {}
                }
            }
        }
        (a, b) if a == b => {}
        (a, b) => out.push(Change::Modified {
            path: path.to_string(),
            old: a.clone(),
            new: b.clone(),
        }),
    }
}

/// Render a list of changes as a unified-diff-ish text block.
pub fn render_text(changes: &[Change]) -> String {
    let mut s = String::new();
    for c in changes {
        match c {
            Change::Added { path, value } => {
                s.push_str(&format!("+ {path}: {}\n", short(value)));
            }
            Change::Removed { path, value } => {
                s.push_str(&format!("- {path}: {}\n", short(value)));
            }
            Change::Modified { path, old, new } => {
                s.push_str(&format!("~ {path}: {} → {}\n", short(old), short(new)));
            }
        }
    }
    s
}

fn short(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_else(|_| "<?>".into());
    if s.len() > 80 {
        format!("{}…", &s[..77])
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_returns_empty() {
        let a = json!({"x": 1, "y": [1, 2, 3]});
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn added_field_detected() {
        let a = json!({"x": 1});
        let b = json!({"x": 1, "y": 2});
        let d = diff(&a, &b);
        assert_eq!(d.len(), 1);
        assert!(matches!(&d[0], Change::Added { path, .. } if path == "y"));
    }

    #[test]
    fn removed_field_detected() {
        let a = json!({"x": 1, "y": 2});
        let b = json!({"x": 1});
        let d = diff(&a, &b);
        assert!(matches!(&d[0], Change::Removed { path, .. } if path == "y"));
    }

    #[test]
    fn modified_scalar_detected() {
        let a = json!({"x": 1});
        let b = json!({"x": 2});
        let d = diff(&a, &b);
        assert!(matches!(&d[0], Change::Modified { path, .. } if path == "x"));
    }

    #[test]
    fn nested_path_dots_and_brackets() {
        let a = json!({"spec": {"containers": [{"image": "nginx:1"}]}});
        let b = json!({"spec": {"containers": [{"image": "nginx:2"}]}});
        let d = diff(&a, &b);
        assert_eq!(d.len(), 1);
        assert!(matches!(&d[0], Change::Modified { path, .. }
            if path == "spec.containers[0].image"));
    }

    #[test]
    fn render_text_includes_markers() {
        let changes = vec![
            Change::Added {
                path: "x".into(),
                value: json!(1),
            },
            Change::Removed {
                path: "y".into(),
                value: json!(2),
            },
            Change::Modified {
                path: "z".into(),
                old: json!(3),
                new: json!(4),
            },
        ];
        let s = render_text(&changes);
        assert!(s.contains("+ x: 1"));
        assert!(s.contains("- y: 2"));
        assert!(s.contains("~ z: 3 → 4"));
    }
}
