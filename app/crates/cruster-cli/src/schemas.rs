//! Compiled-in schema registry.
//!
//! Each `cruster get <kind>` / `cruster describe` / etc. ships a JSON
//! schema. `cruster schema <verb>` looks up the schema by verb name.

const fn pair(name: &'static str, body: &'static str) -> (&'static str, &'static str) {
    (name, body)
}

pub const SCHEMAS: &[(&str, &str)] = &[
    pair("get-pod", include_str!("../schemas/get-pod.schema.json")),
    pair(
        "get-deployment",
        include_str!("../schemas/get-deployment.schema.json"),
    ),
    pair(
        "get-service",
        include_str!("../schemas/get-service.schema.json"),
    ),
    pair("get-node", include_str!("../schemas/get-node.schema.json")),
    pair(
        "get-event",
        include_str!("../schemas/get-event.schema.json"),
    ),
    pair(
        "get-configmap",
        include_str!("../schemas/get-configmap.schema.json"),
    ),
    pair(
        "get-secret",
        include_str!("../schemas/get-secret.schema.json"),
    ),
    pair(
        "get-namespace",
        include_str!("../schemas/get-namespace.schema.json"),
    ),
    pair("describe", include_str!("../schemas/describe.schema.json")),
    pair("logs", include_str!("../schemas/logs.schema.json")),
    pair("events", include_str!("../schemas/events.schema.json")),
    pair("diff", include_str!("../schemas/diff.schema.json")),
    pair("timeline", include_str!("../schemas/timeline.schema.json")),
];

pub fn lookup(verb: &str) -> Option<&'static str> {
    SCHEMAS
        .iter()
        .find_map(|(n, body)| (*n == verb).then_some(*body))
}

pub fn verb_names() -> Vec<&'static str> {
    SCHEMAS.iter().map(|(n, _)| *n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_pod_schema() {
        let s = lookup("get-pod").expect("present");
        let v: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(v["title"], "cruster get pod (NDJSON record)");
    }

    #[test]
    fn lookup_misses_unknown_verb() {
        assert!(lookup("get-quark").is_none());
    }

    #[test]
    fn every_listed_verb_has_valid_json() {
        for (name, body) in SCHEMAS {
            let _: serde_json::Value =
                serde_json::from_str(body).unwrap_or_else(|e| panic!("schema {name} invalid: {e}"));
        }
    }

    #[test]
    fn all_registered_verbs_present() {
        assert_eq!(SCHEMAS.len(), 13);
    }
}
