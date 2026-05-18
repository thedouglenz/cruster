//! Shared log signal detection for diagnose verbs.
//!
//! Sibling verbs (#18 init-failure, #19 no-endpoints, #20 pending) reuse
//! these pattern matchers to classify log lines into actionable signals.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSignal {
    AuthFailure,
    ConnectivityFailure,
    PermissionDenied,
    MissingPath,
}

impl LogSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            LogSignal::AuthFailure => "auth_failure",
            LogSignal::ConnectivityFailure => "connectivity_failure",
            LogSignal::PermissionDenied => "permission_denied",
            LogSignal::MissingPath => "missing_path",
        }
    }
}

static AUTH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)401|unauthorized|invalid[\s_-]?(api[_-]?key|token|credential)").unwrap()
});

static CONNECTIVITY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)connection\s+refused|no\s+route\s+to\s+host|i/o\s+timeout").unwrap()
});

static PERMISSION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)permission\s+denied|EACCES|operation\s+not\s+permitted").unwrap()
});

static MISSING_PATH_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)no\s+such\s+file\s+or\s+directory|ENOENT").unwrap());

pub fn detect_signal(line: &str) -> Option<LogSignal> {
    if AUTH_PATTERN.is_match(line) {
        return Some(LogSignal::AuthFailure);
    }
    if CONNECTIVITY_PATTERN.is_match(line) {
        return Some(LogSignal::ConnectivityFailure);
    }
    if PERMISSION_PATTERN.is_match(line) {
        return Some(LogSignal::PermissionDenied);
    }
    if MISSING_PATH_PATTERN.is_match(line) {
        return Some(LogSignal::MissingPath);
    }
    None
}

pub fn detect_signal_in_lines(lines: &[String]) -> Option<(LogSignal, String)> {
    for line in lines.iter().rev() {
        if let Some(signal) = detect_signal(line) {
            return Some((signal, line.clone()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_401_auth_failure() {
        assert_eq!(
            detect_signal("HTTP 401: Unauthorized"),
            Some(LogSignal::AuthFailure)
        );
        assert_eq!(
            detect_signal("LLM returned 401: invalid x-api-key"),
            Some(LogSignal::AuthFailure)
        );
    }

    #[test]
    fn detects_invalid_token() {
        assert_eq!(
            detect_signal("Error: invalid api_key"),
            Some(LogSignal::AuthFailure)
        );
        assert_eq!(
            detect_signal("authentication failed: invalid-token"),
            Some(LogSignal::AuthFailure)
        );
    }

    #[test]
    fn detects_connection_refused() {
        assert_eq!(
            detect_signal("dial tcp: connection refused"),
            Some(LogSignal::ConnectivityFailure)
        );
        assert_eq!(
            detect_signal("no route to host"),
            Some(LogSignal::ConnectivityFailure)
        );
        assert_eq!(
            detect_signal("context deadline exceeded: i/o timeout"),
            Some(LogSignal::ConnectivityFailure)
        );
    }

    #[test]
    fn detects_permission_denied() {
        assert_eq!(
            detect_signal("open /var/run/secrets: permission denied"),
            Some(LogSignal::PermissionDenied)
        );
        assert_eq!(
            detect_signal("EACCES: cannot write to /data"),
            Some(LogSignal::PermissionDenied)
        );
        assert_eq!(
            detect_signal("operation not permitted"),
            Some(LogSignal::PermissionDenied)
        );
    }

    #[test]
    fn detects_missing_path() {
        assert_eq!(
            detect_signal("stat /config/app.yaml: no such file or directory"),
            Some(LogSignal::MissingPath)
        );
        assert_eq!(
            detect_signal("ENOENT: missing /etc/config"),
            Some(LogSignal::MissingPath)
        );
    }

    #[test]
    fn returns_none_for_unmatched() {
        assert_eq!(detect_signal("INFO: server started on port 8080"), None);
        assert_eq!(detect_signal("processing request"), None);
    }

    #[test]
    fn detect_in_lines_finds_last_signal() {
        let lines = vec![
            "starting up...".to_string(),
            "connection refused to database".to_string(),
            "retrying...".to_string(),
            "LLM returned 401: invalid x-api-key".to_string(),
        ];
        let (signal, line) = detect_signal_in_lines(&lines).unwrap();
        assert_eq!(signal, LogSignal::AuthFailure);
        assert!(line.contains("401"));
    }

    #[test]
    fn detect_in_lines_returns_none_when_empty() {
        let lines: Vec<String> = vec![];
        assert!(detect_signal_in_lines(&lines).is_none());
    }
}
