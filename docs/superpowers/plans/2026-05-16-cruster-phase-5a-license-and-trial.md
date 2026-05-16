# Cruster Phase 5A: License loader + trial flow + tier gating

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire the existing `Tier::Free`-hardcoded gates to a real,
on-disk license. Free tier still works without any license file;
Pro/Team features (currently: prompt actions, diagnostic export,
theme switching) honor the loaded tier. Ship `cruster trial` for a
14-day no-CC Pro trial, and `cruster license` for diagnostics.

**Scope guardrail:** This phase ships *license verification*, not
license issuance. The "license server" / payment flow is out of
scope — issuance happens via offline tools (`scripts/issue-license.sh`)
and the binary just verifies whatever it finds at the canonical path.

**Out of scope:**
- Payment integration (Stripe, Lemon Squeezy, etc.).
- License server / web flow for buying.
- Automatic license refresh / phone-home (the spec defers refresh
  to "at most once per 24h when connectivity is available" — fine
  to defer entirely for 5A).
- Multi-cluster / GitOps / change-correlation features. Their gates
  will land in their own implementation phases; here we only fix
  the gates that *already exist in code*.

## Format

License file: `~/.config/cruster/license.toml`. Plain TOML so users
can inspect it; signature lives in a separate field. The shape:

```toml
[license]
tier = "pro"               # one of: pro, team, enterprise, trial-pro
email = "user@example.com"
issued_at = "2026-05-16T00:00:00Z"
expires_at = "2027-05-16T00:00:00Z"

# base64-url, no padding. Ed25519 signature over the canonical
# JSON representation of the [license] table. Omitted for trial
# files (which the binary self-issues — see Task 3).
signature = "..."
```

The binary embeds one ed25519 public key (32 bytes) at build time.
Trial files are recognised by `tier = "trial-pro"` and have **no
signature** — the binary trusts the trial file iff the file path is
the canonical one and `expires_at` is within 14 days of `issued_at`.
Trial fraud is an accepted risk (a determined user can edit the
file; they could also patch the binary).

## Architecture

- `cruster-core::license` — new module. License struct, loader,
  verifier, helpers.
- `cruster-core::tier` — gains a `from_license` constructor and a
  `is_trial()` helper.
- `cruster-cli` — adds `cruster license [show|verify|path]` and
  `cruster trial` subcommands.
- `cruster-tui::app::App::new` — loads the license at startup,
  derives `tier` from it (instead of `Tier::default()`).
- `scripts/issue-license.sh` — POSIX shell helper that uses
  `openssl` or a Rust helper to sign a license payload. Not shipped
  in the binary path.

## Dependencies

- `ed25519-dalek = "2"` — signature verification.
- `base64 = "0.22"` — base64-url encoding.
- `time = "0.3"` already? if not, use chrono (already a workspace dep).

---

## Task 1: License module in cruster-core

- [ ] Create `app/crates/cruster-core/src/license.rs`:

```rust
//! License loading + verification.

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::tier::Tier;

/// Embedded ed25519 public key. Real release builds replace this via
/// `CRUSTER_LICENSE_PUBKEY` env var (build.rs). For dev builds this
/// is a placeholder key whose private half is in `keys/dev-private.pem`
/// (not committed — generated on first use).
const EMBEDDED_PUBKEY_HEX: &str = env!(
    "CRUSTER_LICENSE_PUBKEY_HEX",
);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct License {
    pub tier: String,
    pub email: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug)]
pub enum LoadError {
    NotFound,
    BadToml(String),
    BadSignature,
    Expired,
    InvalidTier(String),
    InvalidTrialWindow,
}

impl License {
    pub fn canonical_path() -> Option<PathBuf> {
        let mut p = dirs::config_dir()?;
        p.push("cruster");
        p.push("license.toml");
        Some(p)
    }

    pub fn load_default() -> Result<Self, LoadError> { ... }
    pub fn load_from(path: &std::path::Path) -> Result<Self, LoadError> { ... }
    pub fn tier(&self) -> Result<Tier, LoadError> { ... }
    pub fn is_trial(&self) -> bool { self.tier == "trial-pro" }
}

pub fn load_tier_or_free() -> Tier {
    License::load_default()
        .and_then(|l| l.tier())
        .unwrap_or(Tier::Free)
}
```

- [ ] Add a `build.rs` that defaults `CRUSTER_LICENSE_PUBKEY_HEX` to
  a dev key when the env var isn't set. The dev key is fine to
  commit because it's only for local development; production builds
  set the env var.

- [ ] Tests: loader returns NotFound when file is missing; rejects
  expired licenses; accepts a valid signature; trial path skips
  signature check; trial path rejects > 14-day window.

- [ ] Update tier.rs: add `Tier::from_str` and recognise `trial-pro`
  as Pro-equivalent (i.e. `has_pro() == true`).

- [ ] Add deps to workspace + cruster-core's Cargo.toml.

- [ ] Commit.

---

## Task 2: `cruster license` CLI subcommand

- [ ] Add `License` command + `LicenseArgs` to cruster-cli args.
  Subcommands:
  - `cruster license show` — print loaded tier, email, expiry.
  - `cruster license verify` — re-verify the file, exit non-zero on failure.
  - `cruster license path` — print the canonical path, nothing else.

- [ ] Wire into `verbs::mod::dispatch`.

- [ ] Tests at the args-parse level.

- [ ] Commit.

---

## Task 3: `cruster trial` CLI subcommand

- [ ] Add `Trial` command (no args). Behavior:
  - If `license.toml` already exists, refuse with a helpful message
    pointing at `cruster license show`.
  - Otherwise, write a 14-day trial license to the canonical path.
    Format: TOML, `tier = "trial-pro"`, `issued_at` = now,
    `expires_at` = now + 14d, no signature.
  - Print "trial activated until <date>; thanks for trying cruster".

- [ ] Tests: idempotency check (refuses to overwrite), file written
  with correct expiry.

- [ ] Commit.

---

## Task 4: TUI App reads tier from license

- [ ] In `cruster-tui::app::App::new`, replace
  `tier: Tier::default()` with `tier: cruster_core::license::load_tier_or_free()`.

- [ ] Smoke verify: existing tests still pass. The behavior change
  is invisible unless a license file is present (which there isn't
  in test environments).

- [ ] Commit.

---

## Task 5: Phase 5A exit verification

- [ ] `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] Manual smoke:
  - No license file → `cruster license show` reports "no license,
    free tier".
  - `cruster trial` writes a file → `cruster license show` reports
    "trial-pro, expires ...".
  - TUI: with the trial file in place, pressing `P` no longer shows
    "Pro feature" — actually arms the leader.
- [ ] Tag `phase-5a-license-and-trial`.

When 5A ships, 5B (distribution: homebrew + cargo install + CI
release artifacts) follows. Then 5C (landing page under `web/`).
