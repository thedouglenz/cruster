//! Doctor check types — shared across crates for CLI and TUI.
//!
//! The actual check implementations live in `cruster-cli::verbs::doctor`
//! since they require async I/O and kube dependencies.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The result of a single preflight check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    /// Machine-readable check name (e.g. `kubeconfig_parses`).
    pub name: String,
    /// Ok/Warn/Error status.
    pub status: CheckStatus,
    /// Human-readable description of what was found.
    pub message: String,
    /// Optional fix hint for non-ok statuses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl CheckResult {
    pub fn ok(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Ok,
            message: message.into(),
            fix: None,
        }
    }

    pub fn warn(
        name: impl Into<String>,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Warn,
            message: message.into(),
            fix: Some(fix.into()),
        }
    }

    pub fn error(
        name: impl Into<String>,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Error,
            message: message.into(),
            fix: Some(fix.into()),
        }
    }
}

/// Severity level for a check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warn,
    Error,
}

impl CheckStatus {
    /// Convert to exit code: ok=0, warn=1, error=2.
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Warn => 1,
            Self::Error => 2,
        }
    }
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warn => write!(f, "warn"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Aggregated doctor report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    /// Overall status (max severity of all checks).
    pub status: CheckStatus,
    /// Current kube context name, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Individual check results.
    pub checks: Vec<CheckResult>,
}

impl DoctorReport {
    pub fn new(checks: Vec<CheckResult>, context: Option<String>) -> Self {
        let status = checks
            .iter()
            .map(|c| c.status)
            .max()
            .unwrap_or(CheckStatus::Ok);
        Self {
            status,
            context,
            checks,
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.status.exit_code()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_result_ok_has_no_fix() {
        let r = CheckResult::ok("test", "all good");
        assert_eq!(r.status, CheckStatus::Ok);
        assert!(r.fix.is_none());
    }

    #[test]
    fn check_result_warn_has_fix() {
        let r = CheckResult::warn("test", "minor issue", "do this");
        assert_eq!(r.status, CheckStatus::Warn);
        assert_eq!(r.fix.as_deref(), Some("do this"));
    }

    #[test]
    fn check_result_error_has_fix() {
        let r = CheckResult::error("test", "bad", "fix it");
        assert_eq!(r.status, CheckStatus::Error);
        assert_eq!(r.fix.as_deref(), Some("fix it"));
    }

    #[test]
    fn check_status_ordering() {
        assert!(CheckStatus::Ok < CheckStatus::Warn);
        assert!(CheckStatus::Warn < CheckStatus::Error);
    }

    #[test]
    fn check_status_exit_codes() {
        assert_eq!(CheckStatus::Ok.exit_code(), 0);
        assert_eq!(CheckStatus::Warn.exit_code(), 1);
        assert_eq!(CheckStatus::Error.exit_code(), 2);
    }

    #[test]
    fn doctor_report_aggregates_to_max_severity() {
        let checks = vec![
            CheckResult::ok("a", "ok"),
            CheckResult::warn("b", "warn", "fix"),
            CheckResult::ok("c", "ok"),
        ];
        let report = DoctorReport::new(checks, None);
        assert_eq!(report.status, CheckStatus::Warn);
    }

    #[test]
    fn doctor_report_empty_checks_is_ok() {
        let report = DoctorReport::new(vec![], None);
        assert_eq!(report.status, CheckStatus::Ok);
    }

    #[test]
    fn check_status_roundtrips_json() {
        for status in [CheckStatus::Ok, CheckStatus::Warn, CheckStatus::Error] {
            let s = serde_json::to_string(&status).unwrap();
            let back: CheckStatus = serde_json::from_str(&s).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn check_result_json_omits_null_fix() {
        let r = CheckResult::ok("test", "message");
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("fix"));
    }

    #[test]
    fn doctor_report_json_includes_context_when_present() {
        let report = DoctorReport::new(vec![], Some("my-cluster".into()));
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("my-cluster"));
    }
}
