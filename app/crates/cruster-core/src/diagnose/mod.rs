//! Diagnostic modules for analyzing Kubernetes resource problems.
//!
//! Each submodule provides pure functions that take Kubernetes data
//! structures and return structured diagnoses. No I/O happens here.
//!
//! - `crashloop`: CrashLoopBackOff pod diagnosis (#17)
//! - `log_signals`: Shared log pattern matching (reused by #18/#19/#20)

pub mod crashloop;
pub mod log_signals;

pub use crashloop::{diagnose, CrashloopReason, Evidence, WhyCrashloop};
pub use log_signals::{detect_signal, detect_signal_in_lines, LogSignal};
