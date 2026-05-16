//! `cruster help-json` — machine-readable command tree.
//!
//! Agents call this at startup to discover the verb surface. The
//! format intentionally mirrors a subset of OpenAPI: a list of
//! commands, each with a name, description, parameter list, and the
//! JSON schema(s) of their structured output. Inlining schemas means
//! an agent can discover everything it needs with a single call.

use std::collections::BTreeMap;

use clap::CommandFactory;
use serde::Serialize;
use serde_json::Value;

use crate::args::Cli;
use crate::schemas::SCHEMAS;

#[derive(Serialize)]
struct CommandDoc {
    name: String,
    about: String,
    flags: Vec<FlagDoc>,
    /// Inline JSON schemas for this command's structured output, keyed
    /// by the same verb names accepted by `cruster schema <verb>`. For
    /// `get`, contains one entry per resource kind (`get-pod`,
    /// `get-deployment`, ...). For single-schema verbs (`describe`,
    /// `logs`, `events`, `diff`), contains one entry whose key matches
    /// the command name. Empty for commands without structured output
    /// (e.g. `theme`, `schema`, `help-json`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    schemas: BTreeMap<String, Value>,
}

#[derive(Serialize)]
struct FlagDoc {
    name: String,
    short: Option<String>,
    long: Option<String>,
    description: String,
    required: bool,
}

pub async fn run() -> anyhow::Result<()> {
    let cmd = Cli::command();
    let mut docs = Vec::new();
    for sub in cmd.get_subcommands() {
        let mut flags = Vec::new();
        for arg in sub.get_arguments() {
            flags.push(FlagDoc {
                name: arg.get_id().to_string(),
                short: arg.get_short().map(|c| c.to_string()),
                long: arg.get_long().map(|s| s.to_string()),
                description: arg.get_help().map(|h| h.to_string()).unwrap_or_default(),
                required: arg.is_required_set(),
            });
        }
        let schemas = schemas_for(sub.get_name());
        docs.push(CommandDoc {
            name: sub.get_name().to_string(),
            about: sub.get_about().map(|a| a.to_string()).unwrap_or_default(),
            flags,
            schemas,
        });
    }
    println!("{}", serde_json::to_string_pretty(&docs)?);
    Ok(())
}

/// Look up the inline schemas that belong to a given subcommand.
///
/// `get` owns all `get-*` schemas (one per resource kind). Other verbs
/// own the single schema whose registry name matches their command
/// name. Verbs with no structured output get an empty map.
fn schemas_for(command: &str) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (name, body) in SCHEMAS {
        let matches = match command {
            "get" => name.starts_with("get-"),
            other => *name == other,
        };
        if !matches {
            continue;
        }
        // SCHEMAS bodies are validated as JSON by a unit test in
        // schemas.rs; parsing here can't realistically fail at runtime.
        let parsed: Value = serde_json::from_str(body).expect("schema is valid JSON");
        out.insert((*name).to_string(), parsed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_command_inlines_all_get_schemas() {
        let s = schemas_for("get");
        assert!(s.contains_key("get-pod"));
        assert!(s.contains_key("get-deployment"));
        assert!(s.contains_key("get-namespace"));
        // sanity: every key is prefixed `get-`
        for k in s.keys() {
            assert!(k.starts_with("get-"), "unexpected key: {k}");
        }
    }

    #[test]
    fn describe_command_inlines_its_one_schema() {
        let s = schemas_for("describe");
        assert_eq!(s.len(), 1);
        assert!(s.contains_key("describe"));
    }

    #[test]
    fn theme_command_has_no_schemas() {
        assert!(schemas_for("theme").is_empty());
        assert!(schemas_for("help-json").is_empty());
        assert!(schemas_for("schema").is_empty());
    }
}
