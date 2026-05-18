//! `cruster why-crashloop <pod>` — diagnose CrashLoopBackOff pods.
//!
//! Fetches the pod's status and recent logs, runs the diagnostic
//! heuristics, and emits a structured JSON object or human-readable
//! summary depending on the output format.

use std::io::Write;

use anyhow::Context as _;
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};

use cruster_core::diagnose::{diagnose, WhyCrashloop};

use crate::args::{Cli, Format, WhyCrashloopArgs};
use crate::output::{effective_format, stdout_is_tty};

pub async fn run(cli: &Cli, args: &WhyCrashloopArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let ns = args.namespace.as_deref().unwrap_or("default");

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), ns);
    let pod = pod_api.get(&args.pod).await.context("fetching pod")?;

    let logs = fetch_logs(&pod_api, &args.pod, args.tail).await;

    let result = diagnose(&pod, &logs);

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    emit(&mut stdout, format, &result)?;

    Ok(())
}

async fn fetch_logs(api: &Api<Pod>, name: &str, tail: i64) -> Vec<String> {
    let params = LogParams {
        tail_lines: Some(tail),
        ..Default::default()
    };
    match api.logs(name, &params).await {
        Ok(text) => text.lines().map(|s| s.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

fn emit<W: Write>(out: &mut W, format: Format, result: &WhyCrashloop) -> std::io::Result<()> {
    match format {
        Format::Text => {
            writeln!(
                out,
                "{}: {} -- {}",
                result.pod,
                result.reason.as_str(),
                result
                    .evidence
                    .last_log_line
                    .as_deref()
                    .unwrap_or("(no log)")
            )?;
            writeln!(out, "suggested fix: {}", result.suggested_fix)?;
            if let Some(restart_count) = result.evidence.restart_count {
                writeln!(out, "restarts: {}", restart_count)?;
            }
            if let Some(exit_code) = result.evidence.exit_code {
                writeln!(out, "exit code: {}", exit_code)?;
            }
            if let Some(ref err) = result.evidence.apiserver_error {
                writeln!(out, "apiserver: {}", err)?;
            }
            Ok(())
        }
        Format::Json => {
            let json = serde_json::to_string_pretty(result).expect("serialize");
            writeln!(out, "{json}")
        }
        Format::Ndjson | Format::Yaml => {
            let json = serde_json::to_string(result).expect("serialize");
            writeln!(out, "{json}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_text_includes_reason_and_fix() {
        use cruster_core::diagnose::{CrashloopReason, Evidence};
        let result = WhyCrashloop {
            pod: "default/at-foo".to_string(),
            phase: "Running".to_string(),
            reason: CrashloopReason::AuthFailure,
            evidence: Evidence {
                container: Some("agent".to_string()),
                exit_code: Some(1),
                restart_count: Some(5),
                last_log_line: Some("LLM returned 401".to_string()),
                memory_limit: None,
                image: Some("myapp:v1".to_string()),
                apiserver_error: None,
            },
            suggested_fix: "Check API key".to_string(),
        };
        let mut buf = Vec::new();
        emit(&mut buf, Format::Text, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("auth_failure"));
        assert!(output.contains("Check API key"));
        assert!(output.contains("restarts: 5"));
    }

    #[test]
    fn emit_json_produces_valid_json() {
        use cruster_core::diagnose::{CrashloopReason, Evidence};
        let result = WhyCrashloop {
            pod: "default/at-foo".to_string(),
            phase: "Running".to_string(),
            reason: CrashloopReason::OomKilled,
            evidence: Evidence {
                container: Some("agent".to_string()),
                exit_code: Some(137),
                restart_count: Some(3),
                last_log_line: None,
                memory_limit: Some("512Mi".to_string()),
                image: None,
                apiserver_error: None,
            },
            suggested_fix: "Increase memory".to_string(),
        };
        let mut buf = Vec::new();
        emit(&mut buf, Format::Ndjson, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["reason"], "oom_killed");
        assert_eq!(parsed["evidence"]["memory_limit"], "512Mi");
    }
}
