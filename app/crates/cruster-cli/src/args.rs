//! Top-level CLI argument schema (clap derive).

use std::ffi::OsString;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "cruster", about = "Kubernetes TUI + CLI for humans and agents")]
pub struct Cli {
    /// Output format. Defaults to `text` in a TTY, `ndjson` otherwise.
    #[arg(long, short = 'o', global = true)]
    pub format: Option<Format>,

    /// Shorthand for `--format ndjson` plus aggressive field pruning.
    /// Auto-on when stdout is not a TTY.
    #[arg(long, global = true)]
    pub llm: bool,

    /// Disable field pruning (include managedFields, status timestamps,
    /// etc.). Off by default.
    #[arg(long, global = true)]
    pub full: bool,

    /// Hard cap on output size in approximate tokens. Trimmed at record
    /// boundaries with a `truncated: true` marker.
    #[arg(long, global = true)]
    pub budget: Option<usize>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn try_parse<I, T>(argv: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(argv)
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List resources of a given kind.
    Get(GetArgs),
    /// Show detailed information about a single resource.
    Describe(DescribeArgs),
    /// Print pod logs.
    Logs(LogsArgs),
    /// Print recent cluster events.
    Events(EventsArgs),
    /// Print the JSON schema of a verb's structured output.
    Schema(SchemaArgs),
    /// Print machine-readable help (same as `--help --format json`).
    #[command(name = "help-json")]
    HelpJson,
    /// Structural diff between two resources.
    Diff(DiffArgs),
    /// Theme inspection and (Phase 5) installation.
    Theme(ThemeArgs),
    /// Build a diagnostic markdown bundle for a resource.
    Export(ExportArgs),
    /// Inspect the currently loaded license.
    License(LicenseArgs),
    /// Activate a 14-day Pro trial (no credit card, no email required).
    Trial,
    /// Merged chronological event stream for a resource (v1: pod refs only).
    Timeline(TimelineArgs),
    /// What changed in a namespace recently (rotations, rollouts, image
    /// bumps, workload creations).
    Changed(ChangedArgs),
    /// Preflight checks: kubeconfig, kubectl, cluster connectivity, permissions.
    Doctor(DoctorArgs),
    /// Diagnose why a pod is in CrashLoopBackOff.
    #[command(name = "why-crashloop")]
    WhyCrashloop(WhyCrashloopArgs),
    /// Diagnose why a Pod is stuck in Pending state.
    #[command(name = "why-pending")]
    WhyPending(WhyPendingArgs),
}

#[derive(Debug, Parser)]
pub struct ChangedArgs {
    /// Namespace to scan. Required (no all-namespaces in v1).
    #[arg(long, short = 'n')]
    pub namespace: String,
    /// Window to scan back. Duration like `30m`, `2h`, `1d`. Default `30m`.
    #[arg(long, default_value = "30m")]
    pub since: String,
    /// Comma-separated list of change kinds to include. Default: all.
    /// Useful for narrowing: `--kind secret_rotated,image_changed`.
    #[arg(long)]
    pub kind: Option<String>,
}

#[derive(Debug, Parser)]
pub struct TimelineArgs {
    /// Resource reference: `kind/name`, e.g. `pod/at-foo`. v1 supports
    /// pod refs only; other kinds error out with a clear message.
    pub reference: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Window to scan back. Duration like `30m`, `2h`, `1d`. Default `1h`.
    #[arg(long, default_value = "1h")]
    pub since: String,
    /// Comma-separated list of record kinds to include. Default: all.
    /// Useful for narrowing dense timelines (e.g. `--include restarted,oom_killed`).
    #[arg(long)]
    pub include: Option<String>,
}

#[derive(Debug, Parser)]
pub struct LicenseArgs {
    #[command(subcommand)]
    pub sub: LicenseSub,
}

#[derive(Debug, Subcommand)]
pub enum LicenseSub {
    /// Print the loaded license: tier, email, expiry. Exits 0 even
    /// for the "no license, free tier" case.
    Show,
    /// Re-verify the license file. Exits non-zero with the reason if
    /// the file is missing, expired, or has a bad signature.
    Verify,
    /// Print the canonical path the binary reads (and where `cruster
    /// trial` writes), nothing else. Useful for scripting.
    Path,
}

#[derive(Debug, Parser)]
pub struct ExportArgs {
    /// Resource reference: `kind/name`, e.g. `pod/nginx`.
    pub reference: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Write the markdown to this file instead of stdout.
    #[arg(long, short = 'o')]
    pub output: Option<String>,
    /// Tail this many log lines (Pod resources only). Default 100.
    #[arg(long, default_value = "100")]
    pub tail: i64,
}

#[derive(Debug, Parser)]
pub struct ThemeArgs {
    #[command(subcommand)]
    pub sub: ThemeSub,
}

#[derive(Debug, Subcommand)]
pub enum ThemeSub {
    /// List bundled theme names.
    List,
    /// Print resolved colors for a bundled theme.
    Preview {
        /// Theme name (e.g. `dark`, `gruvbox`).
        name: String,
    },
    /// Install a theme from a URL (Pro feature; not implemented in v1).
    Install {
        /// URL of the theme TOML file.
        url: String,
    },
}

#[derive(Debug, Parser)]
pub struct DiffArgs {
    /// First reference: `kind/name`
    pub a: String,
    /// Second reference: `kind/name`
    pub b: String,
    /// Convenience: same namespace for both refs.
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Namespace for `a` specifically (overrides --namespace).
    #[arg(long)]
    pub a_namespace: Option<String>,
    /// Namespace for `b` specifically (overrides --namespace).
    #[arg(long)]
    pub b_namespace: Option<String>,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
pub enum Format {
    Text,
    Json,
    Ndjson,
    Yaml,
}

#[derive(Debug, Parser)]
pub struct GetArgs {
    /// Kind to list: pods, deployments, services, nodes, events,
    /// configmaps, secrets, namespaces. Aliases (po, deploy, svc, no,
    /// ev, cm, sec, ns) are accepted.
    pub kind: String,
    /// Optional resource name to filter to.
    pub name: Option<String>,
    /// Namespace. Defaults to all-namespaces.
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Label selector (e.g. `app=web,tier!=frontend`).
    #[arg(long, short = 'l')]
    pub selector: Option<String>,
}

#[derive(Debug, Parser)]
pub struct DescribeArgs {
    /// Resource reference: `kind/name`, e.g. `pod/nginx`.
    pub reference: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
}

#[derive(Debug, Parser)]
pub struct LogsArgs {
    /// Pod name (with optional `pod/` prefix).
    pub pod: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Container name within the pod. Required if the pod has multiple
    /// containers and `--all-containers` is not set.
    #[arg(long, short = 'c')]
    pub container: Option<String>,
    /// Stream new log lines as they appear. Incompatible with `--grep*`,
    /// `-A`/`-B`/`-C`, `--previous`, and `--all-containers`.
    #[arg(long, short = 'f')]
    pub follow: bool,
    /// Maximum lines to fetch per source.
    #[arg(long)]
    pub tail: Option<i64>,
    /// Only return lines newer than this duration (e.g. `5m`, `1h`).
    #[arg(long)]
    pub since: Option<String>,
    /// Regex pattern to search for. Repeatable. Format: `[name=]regex`.
    /// If `name=` is omitted, the regex itself is used as the pattern
    /// name. Triggers grep mode: NDJSON `{hit: ...}` records per match
    /// plus a terminal `{summary: ...}` record listing scanned sources
    /// and any patterns that did not fire.
    #[arg(long, value_name = "[name=]regex")]
    pub grep: Vec<String>,
    /// Literal-substring pattern to search for. Repeatable. Format:
    /// `[name=]text`. Equivalent to `--grep` with the text escaped.
    #[arg(long, value_name = "[name=]text")]
    pub grep_literal: Vec<String>,
    /// Lines of context after each match (grep -A).
    #[arg(short = 'A', long = "after-context", default_value_t = 0)]
    pub after_context: usize,
    /// Lines of context before each match (grep -B).
    #[arg(short = 'B', long = "before-context", default_value_t = 0)]
    pub before_context: usize,
    /// Lines of context around each match (grep -C; sets both -A and -B).
    #[arg(long = "context", default_value_t = 0)]
    pub context: usize,
    /// Also scan the previous container's logs (the last crash's stream).
    #[arg(long)]
    pub previous: bool,
    /// Scan every container in the pod, not just the default / `--container`.
    #[arg(long)]
    pub all_containers: bool,
}

#[derive(Debug, Parser)]
pub struct EventsArgs {
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Filter to events involving the given resource: `kind/name`.
    #[arg(long)]
    pub resource: Option<String>,
    /// Max events to return.
    #[arg(long, default_value = "100")]
    pub limit: usize,
}

#[derive(Debug, Parser)]
pub struct SchemaArgs {
    /// Verb to print the schema for, e.g. `get-pod`, `describe`, `logs`.
    pub verb: String,
}

#[derive(Debug, Parser)]
pub struct DoctorArgs {
    /// Output JSON instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct WhyCrashloopArgs {
    /// Pod name (without kind prefix; this verb is pod-specific).
    pub pod: String,
    /// Namespace. Defaults to the default namespace.
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
    /// Number of log lines to fetch. Default 50.
    #[arg(long, default_value = "50")]
    pub tail: i64,
}

#[derive(Debug, Parser)]
pub struct WhyPendingArgs {
    /// Pod reference: `pod/<name>` or just `<name>`.
    pub reference: String,
    #[arg(long, short = 'n')]
    pub namespace: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_pods() {
        let cli = Cli::try_parse(["cruster", "get", "pods"]).unwrap();
        let Command::Get(args) = cli.command else {
            panic!("expected Get")
        };
        assert_eq!(args.kind, "pods");
    }

    #[test]
    fn parses_logs_with_follow_and_grep() {
        let cli = Cli::try_parse(["cruster", "logs", "nginx", "-f", "--grep", "error"]).unwrap();
        let Command::Logs(args) = cli.command else {
            panic!("expected Logs")
        };
        assert_eq!(args.pod, "nginx");
        assert!(args.follow);
        assert_eq!(args.grep, vec!["error".to_string()]);
    }

    #[test]
    fn parses_logs_with_multiple_named_greps_and_context() {
        let cli = Cli::try_parse([
            "cruster",
            "logs",
            "nginx",
            "-n",
            "default",
            "--grep",
            "auth=(?i)\\b(401|403)\\b",
            "--grep",
            "panic=panic:|fatal error:",
            "--grep-literal",
            "host=connection refused",
            "-A",
            "3",
            "-B",
            "2",
            "--previous",
            "--all-containers",
        ])
        .unwrap();
        let Command::Logs(args) = cli.command else {
            panic!("expected Logs")
        };
        assert_eq!(args.grep.len(), 2);
        assert_eq!(args.grep_literal.len(), 1);
        assert_eq!(args.after_context, 3);
        assert_eq!(args.before_context, 2);
        assert!(args.previous);
        assert!(args.all_containers);
    }

    #[test]
    fn llm_flag_propagates() {
        let cli = Cli::try_parse(["cruster", "--llm", "get", "pods"]).unwrap();
        assert!(cli.llm);
    }

    #[test]
    fn format_explicit_overrides_auto() {
        let cli = Cli::try_parse(["cruster", "--format", "yaml", "get", "pods"]).unwrap();
        assert!(matches!(cli.format, Some(Format::Yaml)));
    }

    #[test]
    fn budget_parses_to_usize() {
        let cli = Cli::try_parse(["cruster", "--budget", "1500", "get", "pods"]).unwrap();
        assert_eq!(cli.budget, Some(1500));
    }
}
