# Cruster Phase 5B: Distribution — cargo install + homebrew + CI releases

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make cruster installable through the channels SREs actually
use: `cargo install` for Rust folks, `brew install` for Mac users, and
prebuilt release tarballs for everyone else. Wire a GitHub Actions
workflow that builds + uploads release artifacts whenever a `v*` tag
is pushed.

**Scope guardrail:** Ship the *files* needed for each distribution
channel. Actual publishing (cargo publish, brew tap commit, GitHub
release) is a one-shot operation the user runs when they're ready
— this phase just makes those operations work.

**Out of scope:**
- Publishing to crates.io (user decision: name, public package vs
  private). The metadata is correct so `cargo publish` works when
  the user wants to run it.
- Windows support (not in spec; arm64 macOS + x86_64 macOS + x86_64
  Linux only).
- Docker image (not in spec).
- Self-update mechanism (deferred per spec "open questions").
- Codesigning / notarisation for macOS (deferred; v1 ships unsigned).

## Artifacts shipped this phase

1. **Cargo metadata** so `cargo install --git ...` produces a working
   `cruster` binary. Top-level `description`, `homepage`,
   `repository`, `readme`, `keywords`, `categories`.
2. **`.github/workflows/release.yml`** — triggered by `v*` tags.
   Builds release binaries on macOS-13 (x86_64), macOS-14 (arm64),
   and ubuntu-latest (x86_64). Tarballs them. Uploads to a draft
   GitHub release.
3. **`.github/workflows/ci.yml`** — runs on push/PR. fmt + clippy +
   tests on ubuntu-latest. No release artifacts.
4. **`Formula/cruster.rb`** — homebrew formula stub. Three URL
   variants (arm64 macOS, x86_64 macOS, x86_64 linux) with
   placeholder SHA256s that get filled in by the release workflow
   (or by hand after each release).
5. **`docs/RELEASING.md`** — the runbook: how to cut a release, where
   to copy the SHA256s, what to push to the homebrew tap.
6. **README Distribution section** — install instructions for each
   channel.

---

## Task 1: Cargo metadata for `cargo install`

- [ ] Add a top-level `[package]` block to `app/crates/cruster-bin/
  Cargo.toml`: `description`, `homepage`, `repository`, `readme`,
  `keywords`, `categories`. Mirror the `cruster-cli` / `cruster-tui`
  package metadata.
- [ ] Add `description = ...` to the workspace `[workspace.package]`
  block if it makes the per-crate definitions cleaner.
- [ ] Local smoke: `cargo install --path app/crates/cruster-bin
  --locked --root /tmp/cruster-test && ls -lh /tmp/cruster-test/bin/cruster
  && /tmp/cruster-test/bin/cruster --version`. Confirm the binary
  exists, is named `cruster`, and runs.
- [ ] Commit.

## Task 2: CI workflow (`.github/workflows/ci.yml`)

The existing repo doesn't have CI. Add a minimal one:

- [ ] Triggers: `push` to main, `pull_request`.
- [ ] One job, `ubuntu-latest`. Steps:
  - `actions/checkout@v4`
  - `dtolnay/rust-toolchain@stable` with components `rustfmt`, `clippy`
  - `Swatinem/rust-cache@v2`
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --locked`
- [ ] All steps run in `app/` (the cargo workspace root). Use
  `working-directory: app` at the job or step level.
- [ ] Commit.

## Task 3: Release workflow (`.github/workflows/release.yml`)

- [ ] Triggers: `push` to tags matching `v*`.
- [ ] Matrix job over three targets:
  - `aarch64-apple-darwin` on `macos-14`
  - `x86_64-apple-darwin` on `macos-13`
  - `x86_64-unknown-linux-gnu` on `ubuntu-latest`
- [ ] Steps per matrix entry:
  - checkout
  - install Rust toolchain + target
  - cache
  - `cd app && cargo build --release --target ${{ matrix.target }}`
  - tarball: `cruster-<version>-<target>.tar.gz` containing the
    `cruster` binary, the README, and the LICENSE (if present;
    skipped otherwise).
  - upload tarball as a workflow artifact.
- [ ] A final `release` job that:
  - downloads all artifacts
  - runs `gh release create v${VERSION} --draft --notes "..."
    cruster-*.tar.gz`
  - requires `permissions: contents: write`.
- [ ] Commit.

## Task 4: Homebrew formula + tap docs

- [ ] Create `Formula/cruster.rb`:

```ruby
class Cruster < Formula
  desc "Kubernetes TUI + CLI with agent-friendly diagnostics"
  homepage "https://github.com/thedouglenz/cruster"
  version "0.1.0"
  license :cannot_represent  # proprietary; brew has no SPDX for this

  on_macos do
    on_arm do
      url "https://github.com/thedouglenz/cruster/releases/download/v0.1.0/cruster-0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_RELEASE_SHA256"
    end
    on_intel do
      url "https://github.com/thedouglenz/cruster/releases/download/v0.1.0/cruster-0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_RELEASE_SHA256"
    end
  end

  on_linux do
    url "https://github.com/thedouglenz/cruster/releases/download/v0.1.0/cruster-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "REPLACE_WITH_RELEASE_SHA256"
  end

  def install
    bin.install "cruster"
  end

  test do
    assert_match "cruster", shell_output("#{bin}/cruster --help")
  end
end
```

- [ ] Run `ruby -c Formula/cruster.rb` to syntax-check.
- [ ] Commit.

## Task 5: `docs/RELEASING.md` runbook + README updates

- [ ] Write a step-by-step `docs/RELEASING.md`:
  1. Update `version` in `app/Cargo.toml`'s `[workspace.package]`.
  2. `cargo build --release` to refresh `Cargo.lock`.
  3. Commit `bump to vX.Y.Z`.
  4. `git tag vX.Y.Z && git push --tags`.
  5. Wait for the release workflow to finish; GitHub release is
     created as a draft.
  6. Pull the three SHA256s out of the release artifacts.
  7. Update `Formula/cruster.rb` (URL version + three sha256s).
  8. If you maintain a homebrew tap repo separately, copy the
     formula there and tag it.
  9. Promote the draft release to "latest".
- [ ] Add a "Distribution" section to the top-level README listing:
  - `cargo install --git https://github.com/thedouglenz/cruster.git --bin cruster --locked`
  - `brew tap thedouglenz/cruster && brew install cruster`
  - "Download prebuilt binary" pointing at the releases page.
- [ ] Bump README status to Phase 5B.
- [ ] Commit.

## Task 6: Phase 5B exit verification

- [ ] `cargo build --release` still clean.
- [ ] `cargo install --path app/crates/cruster-bin --locked --root /tmp/cruster-install-smoke`
  exits 0; `/tmp/cruster-install-smoke/bin/cruster --help` runs.
- [ ] `ruby -c Formula/cruster.rb` reports OK.
- [ ] YAML lint on the two workflow files: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"`.
- [ ] Tag `phase-5b-distribution`.

When 5B ships, 5C (landing page under `web/`) follows.
