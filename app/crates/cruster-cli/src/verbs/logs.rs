//! `cruster logs <pod>` — print pod logs.

use std::io::Write;
use std::str::FromStr;

use anyhow::Context;
use futures::{AsyncBufReadExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use serde::Serialize;

use crate::args::{Cli, Format, LogsArgs};
use crate::output::{effective_format, stdout_is_tty};

#[derive(Serialize)]
struct LogRecord<'a> {
    pod: &'a str,
    container: Option<&'a str>,
    line: &'a str,
}

pub async fn run(cli: &Cli, args: &LogsArgs) -> anyhow::Result<()> {
    let pod_name = args.pod.strip_prefix("pod/").unwrap_or(&args.pod);
    let ns = args
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--namespace is required for logs"))?;

    let client = Client::try_default().await?;
    let api: Api<Pod> = Api::namespaced(client, ns);

    let mut params = LogParams {
        follow: args.follow,
        container: args.container.clone(),
        ..Default::default()
    };
    params.tail_lines = args.tail;
    if let Some(since) = &args.since {
        params.since_seconds = Some(parse_duration_seconds(since)?);
    }

    let stream = api
        .log_stream(pod_name, &params)
        .await
        .context("opening log stream")?;
    let mut reader = stream.lines();

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();

    while let Some(line) = reader.try_next().await? {
        if let Some(grep) = &args.grep {
            if !line.contains(grep) {
                continue;
            }
        }
        match format {
            Format::Text => writeln!(stdout, "{line}")?,
            Format::Ndjson | Format::Json | Format::Yaml => {
                let rec = LogRecord {
                    pod: pod_name,
                    container: args.container.as_deref(),
                    line: &line,
                };
                let s = serde_json::to_string(&rec).expect("serialize");
                writeln!(stdout, "{s}")?;
            }
        }
        stdout.flush()?;
    }
    Ok(())
}

/// Parse a `5m`, `2h`, `30s`, `1d` duration into seconds.
fn parse_duration_seconds(s: &str) -> anyhow::Result<i64> {
    if s.is_empty() {
        anyhow::bail!("empty duration");
    }
    let (num, suffix) = s.split_at(s.len() - 1);
    let n: i64 = i64::from_str(num).context("duration number")?;
    match suffix {
        "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        "d" => Ok(n * 86400),
        _ => anyhow::bail!("unknown duration suffix: {suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration_seconds("30s").unwrap(), 30);
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("2h").unwrap(), 7200);
        assert_eq!(parse_duration_seconds("1d").unwrap(), 86400);
    }

    #[test]
    fn rejects_bogus_durations() {
        assert!(parse_duration_seconds("5x").is_err());
        assert!(parse_duration_seconds("abc").is_err());
        assert!(parse_duration_seconds("").is_err());
    }
}
