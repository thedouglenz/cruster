//! `cruster why-init-failure <pod>` — diagnose init-container failures.
//!
//! Fetches the pod's status and recent init-container logs, runs the
//! diagnostic heuristics, and emits a structured JSON object or
//! human-readable summary depending on the output format.

use std::io::Write;

use anyhow::{bail, Context as _};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use serde_json::json;

use cruster_core::diagnose::{diagnose_init, InitDiagnoseError, WhyInitFailure};

use crate::args::{Cli, Format, WhyInitFailureArgs};
use crate::output::{effective_format, stdout_is_tty};

pub async fn run(cli: &Cli, args: &WhyInitFailureArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;
    let ns = args.namespace.as_deref().unwrap_or("default");

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), ns);
    let pod = pod_api.get(&args.pod).await.context("fetching pod")?;

    let init_container_name = find_failing_init_name(&pod);
    let logs = if let Some(ref name) = init_container_name {
        fetch_init_logs(&pod_api, &args.pod, name, args.tail).await
    } else {
        Vec::new()
    };

    let result = match diagnose_init(&pod, &logs) {
        Ok(diag) => diag,
        Err(InitDiagnoseError::NoInitContainers) => {
            let err = json!({
                "error": "pod has no init containers; nothing to diagnose"
            });
            let format = effective_format(cli.format, cli.llm, stdout_is_tty());
            let mut stdout = std::io::stdout().lock();
            match format {
                Format::Text => {
                    writeln!(
                        stdout,
                        "{}: has no init containers; nothing to diagnose",
                        args.pod
                    )?;
                }
                _ => {
                    writeln!(stdout, "{}", serde_json::to_string(&err)?)?;
                }
            }
            bail!("pod has no init containers; nothing to diagnose");
        }
        Err(InitDiagnoseError::AllInitSucceeded) => {
            let err = json!({
                "error": "all init containers succeeded; nothing to diagnose"
            });
            let format = effective_format(cli.format, cli.llm, stdout_is_tty());
            let mut stdout = std::io::stdout().lock();
            match format {
                Format::Text => {
                    writeln!(
                        stdout,
                        "{}: all init containers succeeded; nothing to diagnose",
                        args.pod
                    )?;
                }
                _ => {
                    writeln!(stdout, "{}", serde_json::to_string(&err)?)?;
                }
            }
            bail!("all init containers succeeded; nothing to diagnose");
        }
    };

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();
    emit(&mut stdout, format, &result)?;

    Ok(())
}

fn find_failing_init_name(pod: &Pod) -> Option<String> {
    let status = pod.status.as_ref()?;
    let init_statuses = status.init_container_statuses.as_ref()?;

    for st in init_statuses {
        let state = st.state.as_ref();
        let terminated = state.and_then(|s| s.terminated.as_ref());

        if let Some(term) = terminated {
            if term.exit_code != 0 {
                return Some(st.name.clone());
            }
            continue;
        }

        let waiting = state.and_then(|s| s.waiting.as_ref());
        if waiting.is_some() || st.restart_count > 0 {
            return Some(st.name.clone());
        }
    }

    init_statuses.first().map(|s| s.name.clone())
}

async fn fetch_init_logs(
    api: &Api<Pod>,
    pod_name: &str,
    container: &str,
    tail: i64,
) -> Vec<String> {
    let params = LogParams {
        container: Some(container.to_string()),
        tail_lines: Some(tail),
        ..Default::default()
    };
    match api.logs(pod_name, &params).await {
        Ok(text) => text.lines().map(|s| s.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

fn emit<W: Write>(out: &mut W, format: Format, result: &WhyInitFailure) -> std::io::Result<()> {
    match format {
        Format::Text => {
            writeln!(
                out,
                "{}: {} (init[{}]={}) -- {}",
                result.pod,
                result.reason.as_str(),
                result.init_index,
                result.init_container,
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
            if let Some(ref image) = result.evidence.image {
                writeln!(out, "image: {}", image)?;
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
    use cruster_core::diagnose::{InitEvidence, InitFailureReason};

    #[test]
    fn emit_text_includes_reason_and_fix() {
        let result = WhyInitFailure {
            pod: "default/at-foo".to_string(),
            init_container: "wait-for-db".to_string(),
            init_index: 0,
            reason: InitFailureReason::AuthFailure,
            evidence: InitEvidence {
                exit_code: Some(1),
                restart_count: Some(5),
                last_log_line: Some("LLM returned 401".to_string()),
                memory_limit: None,
                image: Some("init:v1".to_string()),
                apiserver_error: None,
                stuck_for: None,
            },
            suggested_fix: "Check API key".to_string(),
        };
        let mut buf = Vec::new();
        emit(&mut buf, Format::Text, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("auth_failure"));
        assert!(output.contains("Check API key"));
        assert!(output.contains("restarts: 5"));
        assert!(output.contains("init[0]=wait-for-db"));
    }

    #[test]
    fn emit_json_produces_valid_json() {
        let result = WhyInitFailure {
            pod: "default/at-foo".to_string(),
            init_container: "wait-for-db".to_string(),
            init_index: 0,
            reason: InitFailureReason::InitOomKilled,
            evidence: InitEvidence {
                exit_code: Some(137),
                restart_count: Some(3),
                last_log_line: None,
                memory_limit: Some("256Mi".to_string()),
                image: None,
                apiserver_error: None,
                stuck_for: None,
            },
            suggested_fix: "Increase memory".to_string(),
        };
        let mut buf = Vec::new();
        emit(&mut buf, Format::Ndjson, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["reason"], "init_oom_killed");
        assert_eq!(parsed["evidence"]["memory_limit"], "256Mi");
        assert_eq!(parsed["init_index"], 0);
    }
}
