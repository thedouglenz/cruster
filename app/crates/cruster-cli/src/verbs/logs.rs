//! `cruster logs <pod>` — print pod logs, with optional multi-pattern
//! grep over current + previous + multi-container windows.
//!
//! Two modes:
//!
//! - **Stream mode** (default; no `--grep` / `--grep-literal`). Same as
//!   today: print raw lines in text mode, `{pod, container, line}` in
//!   NDJSON. `--follow` is supported.
//!
//! - **Grep mode** (any `--grep` or `--grep-literal` set). Fetches each
//!   source as a buffered window, scans every line against every
//!   pattern, emits one `{hit: ...}` NDJSON record per match with `-A`
//!   / `-B` context, then a terminal `{summary: ...}` record listing
//!   every source scanned, every pattern that fired, every pattern that
//!   did not, and any truncation. `--follow` is rejected in grep mode
//!   because context windows require buffering.

use std::collections::BTreeMap;
use std::io::Write;
use std::str::FromStr;

use anyhow::{bail, Context};
use futures::{AsyncBufReadExt, TryStreamExt};
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Client};
use regex::Regex;
use serde::Serialize;

use crate::args::{Cli, Format, LogsArgs};
use crate::output::{effective_format, stdout_is_tty};

// ---------- stream-mode record (unchanged shape from v0.1) ----------

#[derive(Serialize)]
struct LineRecord<'a> {
    pod: &'a str,
    container: Option<&'a str>,
    line: &'a str,
}

// ---------- grep-mode types ----------

/// One source we scanned: pod + container + which stream (current or
/// previous crash) + line count + whether we hit a buffer limit.
#[derive(Debug, Clone, Serialize)]
struct SourceId {
    pod: String,
    namespace: String,
    container: String,
    /// `"current"` | `"previous"`
    stream: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SourceScanned {
    #[serde(flatten)]
    id: SourceId,
    lines: usize,
    /// Present only when the source could not be fetched (e.g. no
    /// previous logs because container has never crashed).
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Resolved pattern after parsing `[name=]expr` and (for literal mode)
/// regex-escaping.
#[derive(Debug, Clone)]
struct Pattern {
    name: String,
    /// Original expression as the user wrote it (regex source, or the
    /// literal string before escaping). Echoed in hit records so the
    /// LLM can confirm what fired.
    expr: String,
    regex: Regex,
}

#[derive(Debug, Serialize)]
struct PatternEcho<'a> {
    id: &'a str,
    expr: &'a str,
}

#[derive(Debug, Serialize)]
struct ContextLine<'a> {
    line_offset: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    ts: Option<&'a str>,
    text: &'a str,
}

#[derive(Debug, Serialize)]
struct Hit<'a> {
    source: &'a SourceId,
    pattern: PatternEcho<'a>,
    #[serde(rename = "match")]
    match_: ContextLine<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    before: Vec<ContextLine<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    after: Vec<ContextLine<'a>>,
}

#[derive(Debug, Serialize)]
struct HitEnvelope<'a> {
    hit: Hit<'a>,
}

#[derive(Debug, Serialize)]
struct Summary {
    sources_scanned: Vec<SourceScanned>,
    /// Pattern id → hit count for every pattern that fired at least once.
    patterns_hit: BTreeMap<String, usize>,
    /// Pattern ids that produced zero hits. The negative-evidence field:
    /// lets a calling agent skip re-checking these.
    patterns_unhit: Vec<String>,
    truncated: TruncationFlags,
    ms_total: u128,
}

#[derive(Debug, Serialize)]
struct TruncationFlags {
    /// True if any source hit `--tail` and might have more lines.
    logs: bool,
}

#[derive(Debug, Serialize)]
struct SummaryEnvelope {
    summary: Summary,
}

// ---------- entry point ----------

pub async fn run(cli: &Cli, args: &LogsArgs) -> anyhow::Result<()> {
    let pod_name = args.pod.strip_prefix("pod/").unwrap_or(&args.pod);
    let ns = args
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--namespace is required for logs"))?;

    let has_grep = !args.grep.is_empty() || !args.grep_literal.is_empty();
    let wants_context = args.after_context > 0 || args.before_context > 0 || args.context > 0;

    // Argument compatibility — fail fast with a clear message rather
    // than silently doing the wrong thing.
    if args.follow && has_grep {
        bail!("--follow is incompatible with --grep / --grep-literal (context windows require buffering)");
    }
    if args.follow && (args.previous || args.all_containers) {
        bail!("--follow is incompatible with --previous and --all-containers");
    }
    if args.follow && wants_context {
        bail!("--follow is incompatible with -A / -B / --context");
    }
    if args.container.is_some() && args.all_containers {
        bail!("--container and --all-containers are mutually exclusive");
    }
    if !has_grep && wants_context {
        bail!("-A / -B / --context require at least one --grep or --grep-literal");
    }

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());

    // Parse patterns before connecting — bad regex shouldn't require a
    // reachable cluster to surface.
    let patterns = if has_grep {
        Some(parse_patterns(&args.grep, &args.grep_literal)?)
    } else {
        None
    };

    let client = Client::try_default().await?;
    let api: Api<Pod> = Api::namespaced(client, ns);

    if !has_grep {
        return run_stream(&api, pod_name, args, format).await;
    }

    let patterns = patterns.expect("has_grep implies patterns parsed");
    let containers = resolve_containers(&api, pod_name, args).await?;
    let streams = resolve_streams(args);

    run_grep(
        &api,
        pod_name,
        ns,
        args,
        format,
        &patterns,
        &containers,
        &streams,
    )
    .await
}

// ---------- stream mode (preserves v0.1 behavior) ----------

async fn run_stream(
    api: &Api<Pod>,
    pod_name: &str,
    args: &LogsArgs,
    format: Format,
) -> anyhow::Result<()> {
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

    let mut stdout = std::io::stdout().lock();
    while let Some(line) = reader.try_next().await? {
        match format {
            Format::Text => writeln!(stdout, "{line}")?,
            Format::Ndjson | Format::Json | Format::Yaml => {
                let rec = LineRecord {
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

// ---------- grep mode ----------

#[derive(Debug, Clone, Copy)]
enum Stream {
    Current,
    Previous,
}

impl Stream {
    fn as_str(self) -> &'static str {
        match self {
            Stream::Current => "current",
            Stream::Previous => "previous",
        }
    }
    fn previous(self) -> bool {
        matches!(self, Stream::Previous)
    }
}

fn resolve_streams(args: &LogsArgs) -> Vec<Stream> {
    let mut s = vec![Stream::Current];
    if args.previous {
        // Previous first in the source list because it's the one most
        // likely to contain the crash output the LLM is looking for.
        s.insert(0, Stream::Previous);
    }
    s
}

async fn resolve_containers(
    api: &Api<Pod>,
    pod_name: &str,
    args: &LogsArgs,
) -> anyhow::Result<Vec<String>> {
    if let Some(c) = &args.container {
        return Ok(vec![c.clone()]);
    }
    if args.all_containers {
        let pod = api.get(pod_name).await.context("fetching pod")?;
        let spec = pod.spec.as_ref().context("pod has no spec")?;
        let names: Vec<String> = spec.containers.iter().map(|c| c.name.clone()).collect();
        if names.is_empty() {
            bail!("pod {pod_name} has no containers");
        }
        return Ok(names);
    }
    // Default: let the API pick. We need a name in the source record
    // either way, so fetch the pod to find the single container.
    let pod = api.get(pod_name).await.context("fetching pod")?;
    let spec = pod.spec.as_ref().context("pod has no spec")?;
    match spec.containers.as_slice() {
        [] => bail!("pod {pod_name} has no containers"),
        [only] => Ok(vec![only.name.clone()]),
        many => bail!(
            "pod {pod_name} has {} containers ({}); pass --container <name> or --all-containers",
            many.len(),
            many.iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_grep(
    api: &Api<Pod>,
    pod_name: &str,
    namespace: &str,
    args: &LogsArgs,
    format: Format,
    patterns: &[Pattern],
    containers: &[String],
    streams: &[Stream],
) -> anyhow::Result<()> {
    let started = std::time::Instant::now();

    let before = args.before_context.max(args.context);
    let after = args.after_context.max(args.context);

    // Fetch every (container, stream) source. A 4xx on previous-logs
    // for a container that has never crashed is normal; record it as
    // an `error` on the source rather than bubbling up.
    let mut fetched: Vec<(SourceId, Result<Vec<String>, String>)> = Vec::new();
    for c in containers {
        for s in streams {
            let id = SourceId {
                pod: pod_name.to_string(),
                namespace: namespace.to_string(),
                container: c.clone(),
                stream: s.as_str(),
            };
            let mut params = LogParams {
                container: Some(c.clone()),
                previous: s.previous(),
                timestamps: true,
                ..Default::default()
            };
            params.tail_lines = args.tail;
            if let Some(since) = &args.since {
                params.since_seconds = Some(parse_duration_seconds(since)?);
            }
            let body = api
                .logs(pod_name, &params)
                .await
                .map(|s| s.lines().map(str::to_string).collect::<Vec<_>>())
                .map_err(|e| short_kube_err(&e));
            fetched.push((id, body));
        }
    }

    // Per-pattern hit counter so we can emit `patterns_hit` and
    // `patterns_unhit` deterministically.
    let mut hit_counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in patterns {
        hit_counts.insert(p.name.clone(), 0);
    }

    let mut stdout = std::io::stdout().lock();

    let mut sources_summary: Vec<SourceScanned> = Vec::with_capacity(fetched.len());
    let mut truncated_logs = false;

    for (id, body) in &fetched {
        match body {
            Ok(lines) => {
                let parsed: Vec<(Option<&str>, &str)> = lines.iter().map(|l| split_ts(l)).collect();
                // Tail-imposed truncation hint: if user asked for --tail
                // N and we got exactly N lines back, more lines may exist.
                if let Some(t) = args.tail {
                    if (lines.len() as i64) >= t {
                        truncated_logs = true;
                    }
                }
                for (idx, (ts, text)) in parsed.iter().enumerate() {
                    for p in patterns {
                        if p.regex.is_match(text) {
                            *hit_counts.entry(p.name.clone()).or_insert(0) += 1;
                            let line_offset = (idx as i64) - (parsed.len() as i64);
                            let mut before_ctx = Vec::with_capacity(before);
                            for b in idx.saturating_sub(before)..idx {
                                let (bts, btext) = parsed[b];
                                before_ctx.push(ContextLine {
                                    line_offset: (b as i64) - (parsed.len() as i64),
                                    ts: bts,
                                    text: btext,
                                });
                            }
                            let mut after_ctx = Vec::with_capacity(after);
                            let end = (idx + 1 + after).min(parsed.len());
                            for a in (idx + 1)..end {
                                let (ats, atext) = parsed[a];
                                after_ctx.push(ContextLine {
                                    line_offset: (a as i64) - (parsed.len() as i64),
                                    ts: ats,
                                    text: atext,
                                });
                            }
                            let env = HitEnvelope {
                                hit: Hit {
                                    source: id,
                                    pattern: PatternEcho {
                                        id: &p.name,
                                        expr: &p.expr,
                                    },
                                    match_: ContextLine {
                                        line_offset,
                                        ts: *ts,
                                        text,
                                    },
                                    before: before_ctx,
                                    after: after_ctx,
                                },
                            };
                            emit(&mut stdout, format, &env, |w| write_text_hit(w, &env.hit))?;
                        }
                    }
                }
                sources_summary.push(SourceScanned {
                    id: id.clone(),
                    lines: lines.len(),
                    error: None,
                });
            }
            Err(err) => {
                sources_summary.push(SourceScanned {
                    id: id.clone(),
                    lines: 0,
                    error: Some(err.clone()),
                });
            }
        }
    }

    // Emit summary record.
    let patterns_hit: BTreeMap<String, usize> = hit_counts
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    let patterns_unhit: Vec<String> = hit_counts
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let summary = SummaryEnvelope {
        summary: Summary {
            sources_scanned: sources_summary,
            patterns_hit,
            patterns_unhit,
            truncated: TruncationFlags {
                logs: truncated_logs,
            },
            ms_total: started.elapsed().as_millis(),
        },
    };
    emit(&mut stdout, format, &summary, |w| {
        write_text_summary(w, &summary.summary)
    })?;
    Ok(())
}

fn emit<T, W, F>(out: &mut W, format: Format, record: &T, text_fallback: F) -> std::io::Result<()>
where
    T: Serialize,
    W: Write,
    F: FnOnce(&mut W) -> std::io::Result<()>,
{
    match format {
        Format::Text => text_fallback(out),
        Format::Json | Format::Ndjson | Format::Yaml => {
            // Same one-record-per-line shape for json/ndjson/yaml here;
            // grep mode is a stream of records, not a single array.
            let line = serde_json::to_string(record).expect("serialize");
            writeln!(out, "{line}")
        }
    }
}

fn write_text_hit<W: Write>(out: &mut W, hit: &Hit) -> std::io::Result<()> {
    let header = format!(
        "{}/{} [{}] ({}):",
        hit.source.namespace, hit.source.pod, hit.source.container, hit.source.stream
    );
    writeln!(out, "{header}  pattern={}  offset={}", hit.pattern.id, hit.match_.line_offset)?;
    for c in &hit.before {
        writeln!(out, "   {:>5}  {}", c.line_offset, c.text)?;
    }
    writeln!(out, ">  {:>5}  {}", hit.match_.line_offset, hit.match_.text)?;
    for c in &hit.after {
        writeln!(out, "   {:>5}  {}", c.line_offset, c.text)?;
    }
    writeln!(out, "--")
}

fn write_text_summary<W: Write>(out: &mut W, s: &Summary) -> std::io::Result<()> {
    writeln!(out, "scanned {} source(s) in {} ms", s.sources_scanned.len(), s.ms_total)?;
    for src in &s.sources_scanned {
        match &src.error {
            None => writeln!(
                out,
                "  {}/{} [{}] ({}): {} lines",
                src.id.namespace, src.id.pod, src.id.container, src.id.stream, src.lines
            )?,
            Some(e) => writeln!(
                out,
                "  {}/{} [{}] ({}): error: {}",
                src.id.namespace, src.id.pod, src.id.container, src.id.stream, e
            )?,
        }
    }
    if !s.patterns_hit.is_empty() {
        writeln!(out, "hit:")?;
        for (k, v) in &s.patterns_hit {
            writeln!(out, "  {k}: {v}")?;
        }
    }
    if !s.patterns_unhit.is_empty() {
        writeln!(out, "no hits: {}", s.patterns_unhit.join(", "))?;
    }
    if s.truncated.logs {
        writeln!(out, "warning: one or more sources hit the --tail limit; more lines may exist")?;
    }
    Ok(())
}

// ---------- helpers (pure, unit-testable) ----------

/// Parse `--grep` (regex) and `--grep-literal` (substring) values.
///
/// Each value is `[name=]expr`. `name` must match
/// `[A-Za-z_][A-Za-z0-9_-]*`; if the left of `=` doesn't, the whole
/// value is treated as the expression and the expression itself is
/// used as the pattern name.
fn parse_patterns(grep: &[String], grep_literal: &[String]) -> anyhow::Result<Vec<Pattern>> {
    let mut out = Vec::with_capacity(grep.len() + grep_literal.len());
    for raw in grep {
        let (name, expr) = split_named(raw);
        let regex = Regex::new(&expr)
            .with_context(|| format!("invalid --grep regex for pattern '{name}': {expr}"))?;
        out.push(Pattern { name, expr, regex });
    }
    for raw in grep_literal {
        let (name, expr) = split_named(raw);
        let regex = Regex::new(&regex::escape(&expr))
            .expect("escaped literal is a valid regex");
        out.push(Pattern { name, expr, regex });
    }
    Ok(out)
}

fn split_named(raw: &str) -> (String, String) {
    if let Some(idx) = raw.find('=') {
        let (left, right) = raw.split_at(idx);
        let right = &right[1..];
        if is_valid_name(left) {
            return (left.to_string(), right.to_string());
        }
    }
    (raw.to_string(), raw.to_string())
}

fn is_valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Split a `--timestamps=true` log line into (ts, message). If the
/// first whitespace-separated token doesn't look like an RFC3339
/// timestamp (e.g. the cluster's timestamps were disabled or the line
/// was already stripped), return `(None, full_line)`.
fn split_ts(line: &str) -> (Option<&str>, &str) {
    let Some(space) = line.find(' ') else {
        return (None, line);
    };
    let (head, tail) = line.split_at(space);
    // Cheap RFC3339 sniff: starts with 4 digits + '-'. Avoids pulling
    // in chrono parsing for a hot loop over potentially many lines.
    if head.len() >= 5
        && head.as_bytes()[0].is_ascii_digit()
        && head.as_bytes()[1].is_ascii_digit()
        && head.as_bytes()[2].is_ascii_digit()
        && head.as_bytes()[3].is_ascii_digit()
        && head.as_bytes()[4] == b'-'
    {
        (Some(head), &tail[1..])
    } else {
        (None, line)
    }
}

fn short_kube_err(e: &kube::Error) -> String {
    // Kube errors round-trip as verbose Display; for source.error we
    // want one short line per source, not a JSON dump.
    let s = e.to_string();
    s.lines().next().unwrap_or(&s).to_string()
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

    #[test]
    fn split_named_uses_left_as_name_when_valid() {
        assert_eq!(split_named("auth=401"), ("auth".into(), "401".into()));
        assert_eq!(
            split_named("net-fail=(connection refused)"),
            ("net-fail".into(), "(connection refused)".into())
        );
    }

    #[test]
    fn split_named_falls_back_when_left_is_not_an_identifier() {
        // Left side has a regex metachar — not a valid pattern name,
        // so the whole thing is treated as the expression.
        assert_eq!(
            split_named("(401|403)=msg"),
            ("(401|403)=msg".into(), "(401|403)=msg".into())
        );
        // No '=' at all.
        assert_eq!(
            split_named("just-a-regex"),
            ("just-a-regex".into(), "just-a-regex".into())
        );
    }

    #[test]
    fn split_named_ignores_equals_inside_regex_body() {
        // The first `=` splits; subsequent `=` stays inside the expr.
        let (name, expr) = split_named("kv=foo=bar=baz");
        assert_eq!(name, "kv");
        assert_eq!(expr, "foo=bar=baz");
    }

    #[test]
    fn parses_regex_and_literal_patterns() {
        let pats = parse_patterns(
            &["auth=(?i)\\b401\\b".to_string()],
            &["host=connection refused".to_string()],
        )
        .unwrap();
        assert_eq!(pats.len(), 2);
        assert!(pats[0].regex.is_match("got HTTP 401 back"));
        assert!(!pats[0].regex.is_match("got HTTP 4010 back")); // \b boundary
        // literal containing regex metachars (none here, but escaping
        // matters generally — see next test)
        assert!(pats[1].regex.is_match("connection refused: dial tcp"));
    }

    #[test]
    fn literal_mode_escapes_regex_metachars() {
        // The string `a.b` as a literal must not match `axb`.
        let pats = parse_patterns(&[], &["m=a.b".to_string()]).unwrap();
        assert!(pats[0].regex.is_match("got a.b in logs"));
        assert!(!pats[0].regex.is_match("got axb in logs"));
    }

    #[test]
    fn rejects_invalid_regex() {
        let err = parse_patterns(&["bad=(unclosed".to_string()], &[]).unwrap_err();
        assert!(err.to_string().contains("invalid --grep regex"));
    }

    #[test]
    fn split_ts_separates_rfc3339_prefix() {
        let (ts, text) = split_ts("2026-05-18T14:21:50.123Z hello world");
        assert_eq!(ts, Some("2026-05-18T14:21:50.123Z"));
        assert_eq!(text, "hello world");
    }

    #[test]
    fn split_ts_returns_none_for_untimestamped_line() {
        let (ts, text) = split_ts("just a regular log line");
        assert_eq!(ts, None);
        assert_eq!(text, "just a regular log line");

        let (ts2, text2) = split_ts("INFO something happened");
        assert_eq!(ts2, None);
        assert_eq!(text2, "INFO something happened");
    }

    #[test]
    fn is_valid_name_enforces_identifier_shape() {
        assert!(is_valid_name("auth"));
        assert!(is_valid_name("net-fail"));
        assert!(is_valid_name("_x"));
        assert!(is_valid_name("a1"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("1abc")); // starts with digit
        assert!(!is_valid_name("a b")); // space
        assert!(!is_valid_name("a.b")); // dot
    }

    /// End-to-end scan over a synthetic source, exercising the line
    /// offset math + context window extraction without touching kube.
    #[test]
    fn line_offset_is_negative_from_end_and_context_is_clamped() {
        let lines: Vec<String> = (0..10)
            .map(|i| format!("2026-05-18T14:00:0{i}Z line-{i}"))
            .collect();
        let parsed: Vec<(Option<&str>, &str)> =
            lines.iter().map(|l| split_ts(l)).collect();

        let hits: Vec<i64> = parsed
            .iter()
            .enumerate()
            .filter_map(|(idx, (_, text))| {
                text.contains("line-7").then_some((idx as i64) - (parsed.len() as i64))
            })
            .collect();
        assert_eq!(hits, vec![-3]); // 10 lines, idx 7 → -3

        // Context window clamping at the end of the buffer: idx=9
        // (last line) with after=5 should yield 0 after-context lines.
        let idx = 9;
        let after = 5;
        let end = (idx + 1 + after).min(parsed.len());
        let after_ctx: Vec<usize> = ((idx + 1)..end).collect();
        assert!(after_ctx.is_empty());

        // Clamping at the start: idx=1 with before=5 should yield just 1.
        let idx: usize = 1;
        let before: usize = 5;
        let before_ctx: Vec<usize> = (idx.saturating_sub(before)..idx).collect();
        assert_eq!(before_ctx, vec![0]);
    }

    #[test]
    fn patterns_unhit_is_set_when_pattern_fires_zero_times() {
        let pats = parse_patterns(
            &["panic=panic:".to_string(), "auth=401".to_string()],
            &[],
        )
        .unwrap();
        let lines = [
            "2026-05-18T14:00:00Z startup".to_string(),
            "2026-05-18T14:00:01Z got HTTP 401 unauthorized".to_string(),
        ];
        let parsed: Vec<(Option<&str>, &str)> =
            lines.iter().map(|l| split_ts(l)).collect();
        let mut hit_counts: BTreeMap<String, usize> = BTreeMap::new();
        for p in &pats {
            hit_counts.insert(p.name.clone(), 0);
        }
        for (_, text) in &parsed {
            for p in &pats {
                if p.regex.is_match(text) {
                    *hit_counts.get_mut(&p.name).unwrap() += 1;
                }
            }
        }
        assert_eq!(hit_counts["auth"], 1);
        assert_eq!(hit_counts["panic"], 0);

        let unhit: Vec<&String> = hit_counts
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(k, _)| k)
            .collect();
        assert_eq!(unhit, vec![&"panic".to_string()]);
    }
}
