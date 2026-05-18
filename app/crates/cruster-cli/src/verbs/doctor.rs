//! `cruster doctor` — preflight checks for kubeconfig and cluster connectivity.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use cruster_core::doctor::{CheckResult, CheckStatus, DoctorReport};
use cruster_core::license::License;
use k8s_openapi::api::core::v1::{Namespace, Pod};
use kube::api::ListParams;
use kube::{Api, Client, Config};
use tokio::time::timeout;

use crate::args::DoctorArgs;

const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run(args: &DoctorArgs) -> anyhow::Result<()> {
    let mut checks = Vec::new();
    let mut context_name: Option<String> = None;

    let kubeconfig_result = check_kubeconfig_parses();
    let kubeconfig_ok = kubeconfig_result.status == CheckStatus::Ok;
    checks.push(kubeconfig_result);

    if kubeconfig_ok {
        let (ctx_check, ctx) = check_current_context_set();
        context_name = ctx;
        checks.push(ctx_check);
    } else {
        checks.push(CheckResult::error(
            "current_context_set",
            "skipped: kubeconfig failed to parse",
            "fix kubeconfig first",
        ));
    }

    checks.push(check_kubectl_on_path().await);

    let client = if kubeconfig_ok && context_name.is_some() {
        match build_client().await {
            Ok(c) => {
                checks.push(CheckResult::ok(
                    "apiserver_reachable",
                    "apiserver responded to version request",
                ));
                Some(c)
            }
            Err(e) => {
                checks.push(CheckResult::error(
                    "apiserver_reachable",
                    format!("apiserver unreachable: {e}"),
                    "check network, VPN, or cluster status",
                ));
                None
            }
        }
    } else {
        checks.push(CheckResult::error(
            "apiserver_reachable",
            "skipped: no valid kubeconfig or context",
            "fix kubeconfig first",
        ));
        None
    };

    if let Some(ref c) = client {
        checks.push(check_can_list_pods(c).await);
        checks.push(check_can_list_namespaces(c).await);
        checks.push(check_metrics_server_present(c).await);
    } else {
        for name in [
            "can_list_pods",
            "can_list_namespaces",
            "metrics_server_present",
        ] {
            checks.push(CheckResult::error(
                name,
                "skipped: no cluster connection",
                "fix apiserver connectivity first",
            ));
        }
    }

    checks.push(check_cruster_config_writable());
    checks.push(check_cruster_cache_writable());
    checks.push(check_license_loadable());

    let report = DoctorReport::new(checks, context_name);
    let exit_code = report.exit_code();

    if args.json || !std::io::stdout().is_terminal() {
        let json = serde_json::to_string_pretty(&report)?;
        println!("{json}");
    } else {
        print_human_readable(&report);
    }

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

fn print_human_readable(report: &DoctorReport) {
    let mut stdout = std::io::stdout().lock();
    if let Some(ctx) = &report.context {
        let _ = writeln!(stdout, "Context: {ctx}\n");
    }

    for check in &report.checks {
        let glyph = match check.status {
            CheckStatus::Ok => "\x1b[32m✓\x1b[0m",
            CheckStatus::Warn => "\x1b[33m⚠\x1b[0m",
            CheckStatus::Error => "\x1b[31m✗\x1b[0m",
        };
        let _ = writeln!(stdout, "{glyph} {}: {}", check.name, check.message);
        if let Some(fix) = &check.fix {
            let _ = writeln!(stdout, "  └─ fix: {fix}");
        }
    }

    let _ = writeln!(stdout);
    let summary = match report.status {
        CheckStatus::Ok => "\x1b[32mall checks passed\x1b[0m",
        CheckStatus::Warn => "\x1b[33msome warnings\x1b[0m",
        CheckStatus::Error => "\x1b[31msome checks failed\x1b[0m",
    };
    let _ = writeln!(stdout, "{summary}");
}

fn check_kubeconfig_parses() -> CheckResult {
    match kube::config::Kubeconfig::read() {
        Ok(_) => CheckResult::ok("kubeconfig_parses", "kubeconfig is valid"),
        Err(e) => CheckResult::error(
            "kubeconfig_parses",
            format!("kubeconfig failed to parse: {e}"),
            "check ~/.kube/config or $KUBECONFIG",
        ),
    }
}

fn check_current_context_set() -> (CheckResult, Option<String>) {
    match kube::config::Kubeconfig::read() {
        Ok(kc) => match kc.current_context {
            Some(ctx) => (
                CheckResult::ok("current_context_set", format!("current context: {ctx}")),
                Some(ctx),
            ),
            None => (
                CheckResult::error(
                    "current_context_set",
                    "no current-context set in kubeconfig",
                    "run: kubectl config use-context <context>",
                ),
                None,
            ),
        },
        Err(e) => (
            CheckResult::error(
                "current_context_set",
                format!("could not read kubeconfig: {e}"),
                "check ~/.kube/config or $KUBECONFIG",
            ),
            None,
        ),
    }
}

async fn check_kubectl_on_path() -> CheckResult {
    let output = tokio::process::Command::new("kubectl")
        .arg("version")
        .arg("--client")
        .arg("--short")
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let version = String::from_utf8_lossy(&o.stdout);
            let version = version.trim();
            CheckResult::ok("kubectl_on_path", format!("kubectl found: {version}"))
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            CheckResult::warn(
                "kubectl_on_path",
                format!("kubectl found but returned error: {}", stderr.trim()),
                "check kubectl installation",
            )
        }
        Err(_) => CheckResult::warn(
            "kubectl_on_path",
            "kubectl not found on PATH",
            "install kubectl: https://kubernetes.io/docs/tasks/tools/",
        ),
    }
}

async fn build_client() -> anyhow::Result<Client> {
    let config = Config::infer().await?;
    let client = Client::try_from(config)?;
    timeout(CHECK_TIMEOUT, client.apiserver_version()).await??;
    Ok(client)
}

async fn check_can_list_pods(client: &Client) -> CheckResult {
    let api: Api<Pod> = Api::default_namespaced(client.clone());
    let lp = ListParams::default().limit(1);
    match timeout(CHECK_TIMEOUT, api.list(&lp)).await {
        Ok(Ok(_)) => CheckResult::ok("can_list_pods", "can list pods in default namespace"),
        Ok(Err(e)) => CheckResult::warn(
            "can_list_pods",
            format!("cannot list pods: {e}"),
            "check RBAC permissions for pod listing",
        ),
        Err(_) => CheckResult::warn(
            "can_list_pods",
            "timeout listing pods",
            "check network or cluster load",
        ),
    }
}

async fn check_can_list_namespaces(client: &Client) -> CheckResult {
    let api: Api<Namespace> = Api::all(client.clone());
    let lp = ListParams::default().limit(1);
    match timeout(CHECK_TIMEOUT, api.list(&lp)).await {
        Ok(Ok(_)) => CheckResult::ok("can_list_namespaces", "can list namespaces"),
        Ok(Err(e)) => CheckResult::warn(
            "can_list_namespaces",
            format!("cannot list namespaces: {e}"),
            "check RBAC permissions for namespace listing",
        ),
        Err(_) => CheckResult::warn(
            "can_list_namespaces",
            "timeout listing namespaces",
            "check network or cluster load",
        ),
    }
}

async fn check_metrics_server_present(client: &Client) -> CheckResult {
    let discovery = kube::discovery::Discovery::new(client.clone());
    match timeout(CHECK_TIMEOUT, discovery.run()).await {
        Ok(Ok(d)) => {
            let has_metrics = d.groups().any(|g| g.name() == "metrics.k8s.io");
            if has_metrics {
                CheckResult::ok("metrics_server_present", "metrics.k8s.io API group found")
            } else {
                CheckResult::warn(
                    "metrics_server_present",
                    "metrics.k8s.io API group not found",
                    "install metrics-server for resource usage data",
                )
            }
        }
        Ok(Err(e)) => CheckResult::warn(
            "metrics_server_present",
            format!("API discovery failed: {e}"),
            "check cluster health",
        ),
        Err(_) => CheckResult::warn(
            "metrics_server_present",
            "timeout during API discovery",
            "check network or cluster load",
        ),
    }
}

fn check_cruster_config_writable() -> CheckResult {
    check_dir_writable(
        "cruster_config_writable",
        dirs::config_dir(),
        "cruster",
        "~/.config/cruster/",
    )
}

fn check_cruster_cache_writable() -> CheckResult {
    check_dir_writable(
        "cruster_cache_writable",
        dirs::cache_dir(),
        "cruster",
        "~/.cache/cruster/",
    )
}

fn check_dir_writable(
    name: &str,
    base: Option<PathBuf>,
    subdir: &str,
    display_path: &str,
) -> CheckResult {
    let Some(mut path) = base else {
        return CheckResult::warn(
            name,
            format!("cannot determine {display_path} location"),
            "check XDG_CONFIG_HOME / XDG_CACHE_HOME environment",
        );
    };
    path.push(subdir);

    if !path.exists() {
        if let Err(e) = std::fs::create_dir_all(&path) {
            return CheckResult::warn(
                name,
                format!("cannot create {display_path}: {e}"),
                "check permissions on parent directory",
            );
        }
    }

    let test_file = path.join(".cruster-doctor-test");
    match std::fs::write(&test_file, b"test") {
        Ok(()) => {
            let _ = std::fs::remove_file(&test_file);
            CheckResult::ok(name, format!("{display_path} is writable"))
        }
        Err(e) => CheckResult::warn(
            name,
            format!("{display_path} not writable: {e}"),
            format!("check permissions on {display_path}"),
        ),
    }
}

fn check_license_loadable() -> CheckResult {
    match License::load_default() {
        Ok(l) => match l.tier() {
            Ok(tier) => CheckResult::ok(
                "license_loadable",
                format!("license valid: tier={tier:?}, expires={}", l.expires_at),
            ),
            Err(e) => CheckResult::warn(
                "license_loadable",
                format!("license present but invalid: {e}"),
                "run `cruster license verify` for details",
            ),
        },
        Err(cruster_core::license::LoadError::NotFound) => {
            CheckResult::ok("license_loadable", "no license file (free tier)")
        }
        Err(e) => CheckResult::warn(
            "license_loadable",
            format!("license file error: {e}"),
            "check ~/.config/cruster/license.toml",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn check_kubeconfig_parses_returns_error_for_missing() {
        std::env::set_var("KUBECONFIG", "/nonexistent/path/kubeconfig");
        let result = check_kubeconfig_parses();
        assert_eq!(result.status, CheckStatus::Error);
        assert!(result.fix.is_some());
        std::env::remove_var("KUBECONFIG");
    }

    #[tokio::test]
    async fn check_kubectl_returns_result() {
        let result = check_kubectl_on_path().await;
        assert!(matches!(result.status, CheckStatus::Ok | CheckStatus::Warn));
    }

    #[test]
    fn check_dir_writable_succeeds_for_temp() {
        let tmp = TempDir::new().unwrap();
        let result = check_dir_writable(
            "test_writable",
            Some(tmp.path().to_path_buf()),
            "subdir",
            "test/subdir/",
        );
        assert_eq!(result.status, CheckStatus::Ok);
    }

    #[test]
    fn check_dir_writable_warns_for_readonly() {
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("readonly");
        fs::create_dir(&subdir).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&subdir, fs::Permissions::from_mode(0o444)).unwrap();
        }

        let result = check_dir_writable("test_writable", Some(subdir), "nested", "test/");

        #[cfg(unix)]
        assert_eq!(result.status, CheckStatus::Warn);
    }

    #[test]
    fn check_license_loadable_returns_ok_for_missing() {
        std::env::set_var("HOME", "/nonexistent");
        let result = check_license_loadable();
        assert!(matches!(result.status, CheckStatus::Ok | CheckStatus::Warn));
        std::env::remove_var("HOME");
    }

    #[test]
    fn doctor_report_json_serializes() {
        let checks = vec![
            CheckResult::ok("a", "ok"),
            CheckResult::warn("b", "warning", "fix this"),
        ];
        let report = DoctorReport::new(checks, Some("test-context".into()));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("test-context"));
        assert!(json.contains("\"status\":\"warn\""));
    }

    #[test]
    fn check_result_constructors() {
        let ok = CheckResult::ok("test", "msg");
        assert_eq!(ok.status, CheckStatus::Ok);
        assert!(ok.fix.is_none());

        let warn = CheckResult::warn("test", "msg", "fix");
        assert_eq!(warn.status, CheckStatus::Warn);
        assert_eq!(warn.fix.as_deref(), Some("fix"));

        let error = CheckResult::error("test", "msg", "fix");
        assert_eq!(error.status, CheckStatus::Error);
    }

    #[test]
    fn aggregate_status_reflects_max_severity() {
        let all_ok = vec![CheckResult::ok("a", "ok"), CheckResult::ok("b", "ok")];
        assert_eq!(DoctorReport::new(all_ok, None).status, CheckStatus::Ok);

        let has_warn = vec![CheckResult::ok("a", "ok"), CheckResult::warn("b", "w", "f")];
        assert_eq!(DoctorReport::new(has_warn, None).status, CheckStatus::Warn);

        let has_error = vec![
            CheckResult::ok("a", "ok"),
            CheckResult::warn("b", "w", "f"),
            CheckResult::error("c", "e", "f"),
        ];
        assert_eq!(
            DoctorReport::new(has_error, None).status,
            CheckStatus::Error
        );
    }
}
