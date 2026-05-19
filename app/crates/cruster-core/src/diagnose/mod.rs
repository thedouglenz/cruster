//! Diagnostic modules for analyzing Kubernetes resource problems.
//!
//! Each submodule provides pure functions that take Kubernetes data
//! structures and return structured diagnoses. No I/O happens here.
//!
//! - `crashloop`: CrashLoopBackOff pod diagnosis (#17)
//! - `endpoints`: Why-no-endpoints diagnosis (#22)
//! - `log_signals`: Shared log pattern matching (reused by #18/#19/#20)
//! - `pending`: Pending pod diagnosis (#18)

pub mod crashloop;
pub mod endpoints;
pub mod log_signals;
pub mod pending;

pub use crashloop::{diagnose, CrashloopReason, Evidence, WhyCrashloop};
pub use log_signals::{detect_signal, detect_signal_in_lines, LogSignal};
