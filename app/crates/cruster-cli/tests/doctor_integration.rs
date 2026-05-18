//! Integration tests for `cruster doctor`.

use cruster_core::doctor::DoctorReport;
use std::process::Command;

#[test]
fn doctor_json_produces_valid_json() {
    let output = Command::new("cargo")
        .args(["run", "-p", "cruster-bin", "--", "doctor", "--json"])
        .current_dir(env!("CARGO_MANIFEST_DIR").to_owned() + "/../..")
        .output()
        .expect("failed to run cruster doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let report: DoctorReport = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("failed to parse doctor JSON output: {e}\nstdout: {stdout}\nstderr: {stderr}");
    });

    assert!(
        !report.checks.is_empty(),
        "doctor should return at least one check"
    );
    for check in &report.checks {
        assert!(!check.name.is_empty(), "check name should not be empty");
        assert!(
            !check.message.is_empty(),
            "check message should not be empty"
        );
    }
}

#[test]
fn doctor_exit_code_matches_severity() {
    let output = Command::new("cargo")
        .args(["run", "-p", "cruster-bin", "--", "doctor", "--json"])
        .current_dir(env!("CARGO_MANIFEST_DIR").to_owned() + "/../..")
        .output()
        .expect("failed to run cruster doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: DoctorReport = serde_json::from_str(&stdout).expect("parse JSON");

    let expected_code = report.exit_code();
    let actual_code = output.status.code().unwrap_or(-1);

    assert_eq!(
        expected_code, actual_code,
        "exit code {} should match report severity {:?}",
        actual_code, report.status
    );
}
