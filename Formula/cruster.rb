# typed: false
# frozen_string_literal: true

# Cruster — Kubernetes TUI + CLI with agent-friendly diagnostics.
# https://github.com/thedouglenz/cruster
#
# Release process:
# 1. Bump `version` below.
# 2. Replace the three REPLACE_WITH_SHA256_AFTER_RELEASE placeholders
#    with the SHA256s emitted by the release workflow (they live in
#    the `.tar.gz.sha256` sidecar files attached to the GitHub
#    release).
# 3. Copy this file to your homebrew tap repo (e.g.
#    homebrew-cruster/Formula/cruster.rb) and tag.

class Cruster < Formula
  desc "Kubernetes TUI + CLI with agent-friendly diagnostics"
  homepage "https://github.com/thedouglenz/cruster"
  version "0.1.0"
  # Cruster is proprietary commercial software; homebrew has no SPDX
  # identifier that matches, so we declare a freeform string.
  license "Proprietary"

  # macOS Intel users: build from source with `cargo install --git ...`.
  # macos-13 runners are deprecated on GitHub Actions, so prebuilts
  # are not published for x86_64-apple-darwin.
  on_macos do
    on_arm do
      url "https://github.com/thedouglenz/cruster/releases/download/v#{version}/cruster-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_SHA256_AFTER_RELEASE"
    end
  end

  on_linux do
    url "https://github.com/thedouglenz/cruster/releases/download/v#{version}/cruster-#{version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "REPLACE_WITH_SHA256_AFTER_RELEASE"
  end

  def install
    bin.install "cruster"
    doc.install "README.md"
  end

  test do
    assert_match "cruster", shell_output("#{bin}/cruster --help")
  end
end
