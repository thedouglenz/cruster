# Releasing cruster

A release ships three artifacts:

1. **GitHub release** with macOS arm64 + Intel and Linux x86_64
   tarballs. Built automatically by `.github/workflows/release.yml`
   when a `v*` tag is pushed.
2. **Homebrew formula** (`Formula/cruster.rb`) updated with the new
   version + SHA256s. Optionally mirrored to a separate
   `homebrew-cruster` tap repo.
3. *(future)* Crates.io publication, once Doug decides whether the
   package goes public.

## Pre-flight

- Working tree clean (`git status` shows nothing).
- All tests pass (`cd app && cargo test --workspace`).
- Clippy clean (`cd app && cargo clippy --workspace --all-targets -- -D warnings`).
- README + changelog reflect the version being released.

## Cut the release

```sh
# 1. Decide the version.
NEW_VERSION="0.1.0"

# 2. Bump the workspace version. (One source of truth in app/Cargo.toml.)
sed -i.bak "s/^version = .*/version = \"$NEW_VERSION\"/" app/Cargo.toml
rm app/Cargo.toml.bak

# 3. Refresh the lockfile so the new version propagates.
(cd app && cargo build --release)

# 4. Commit the bump.
git add app/Cargo.toml app/Cargo.lock
git commit -m "release: bump to v$NEW_VERSION"

# 5. Tag and push. The push triggers .github/workflows/release.yml.
git tag "v$NEW_VERSION"
git push origin main "v$NEW_VERSION"
```

## After the release workflow finishes

The release workflow creates a **draft** GitHub release with three
tarballs and three `.sha256` sidecar files attached.

### 1. Pull the SHA256s for the homebrew formula

```sh
gh release view "v$NEW_VERSION" --json assets --jq '.assets[].name'
# Download each .sha256 file:
gh release download "v$NEW_VERSION" --pattern '*.sha256'
cat cruster-${NEW_VERSION}-aarch64-apple-darwin.tar.gz.sha256
cat cruster-${NEW_VERSION}-x86_64-unknown-linux-gnu.tar.gz.sha256
```

> macOS Intel (`x86_64-apple-darwin`) is not built — GitHub Actions
> retired the `macos-13` runners and there is no other free Intel
> macOS path. Intel users build from source via `cargo install`.

### 2. Update `Formula/cruster.rb`

Bump `version`. Replace the two `REPLACE_WITH_SHA256_AFTER_RELEASE`
strings with the SHA256s from above.

Run `ruby -c Formula/cruster.rb` to confirm the file still parses.

```sh
git add Formula/cruster.rb
git commit -m "brew: formula for v$NEW_VERSION"
git push
```

### 3. Mirror to the tap repo (if it exists)

If you maintain a separate `homebrew-cruster` repo as the tap source:

```sh
cp Formula/cruster.rb ../homebrew-cruster/Formula/cruster.rb
(cd ../homebrew-cruster && git add Formula/cruster.rb && \
  git commit -m "cruster v$NEW_VERSION" && git push)
```

### 4. Promote the draft to live

```sh
gh release edit "v$NEW_VERSION" --draft=false --latest
```

## Quick verification

```sh
# Cargo install path
cargo install --git https://github.com/thedouglenz/cruster.git --bin cruster --locked

# Homebrew path (if tap is set up)
brew tap thedouglenz/cruster
brew install cruster

# Direct download path
curl -L https://github.com/thedouglenz/cruster/releases/download/v$NEW_VERSION/cruster-$NEW_VERSION-$(uname -m)-apple-darwin.tar.gz \
  | tar xz
```

Each should produce a working `cruster --help`.

## Rollback

The draft release can be deleted entirely:

```sh
gh release delete "v$NEW_VERSION" --yes
git push --delete origin "v$NEW_VERSION"
git tag -d "v$NEW_VERSION"
```

If the release was already live and consumers may have downloaded
it, prefer a forward-fix release (v$NEW_VERSION+1) over deletion.
