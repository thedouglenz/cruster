# Cruster Phase 6B: Dashboard header & status-bar color treatment

**Date:** 2026-05-17
**Status:** Draft, pending user approval
**Owner:** Doug

## Summary

The cruster TUI's top-of-screen reads as drab compared to k9s. Two
specific surfaces are at fault:

1. **`render_safety_badge`** (`app.rs`) — the single-row env-colored
   status bar. Renders `bg = env_band.<env>`, `fg = Color::Black`. When
   the user's context resolves to `Environment::Unknown` the bg defaults
   to `darkgray` — the result is dark text on dark background, washed
   out, and the field labels (`unknown`, `rw`, `layout:single`) are
   plain text with no semantic accents.

2. **`render_summary`** (`views/dashboard.rs`) — the four-line summary
   band of the dashboard view. Cluster identity, scale counts, and
   K8s version are all painted in `theme.muted_fg` (darkgray by
   default). Only the context name on line 1 is bolded; everything
   else blends into the chrome.

Goal: bring a k9s-grade level of color and information hierarchy to the
header without inventing a new aesthetic. Reuse the existing theme
palette where it already maps to the right semantics; add a small,
additive set of theme keys for what doesn't.

## Non-goals

- Redesigning the trends band, pin tiles, or list-view headers. Header
  changes only.
- Adding an ASCII wordmark, logo art, or other branding chrome.
- Per-theme bundled-color tuning. Defaults must look reasonable on the
  `terminal` theme; the other bundled themes inherit the new keys'
  defaults and can be polished in a follow-up.
- Breaking schema changes to existing themes. All additions are
  `#[serde(default)]` with `default_*` fns.
- Multi-cluster-context display. The bar still shows one context.

## Visual targets

### Top status bar (`render_safety_badge`)

Same single-row env-colored band. Same `bg` semantics — prod still
floods red, dev still glows green, the env signal stays a peripheral
safety cue. Foreground is rebuilt:

```
 │ enginyyr-enginyyr-oidc │ UNKNOWN │ [RW] │ single │
```

- Field separator: `│` glyph in `theme.muted_fg`, replacing `[]` and `·`.
- Context name: **bold**, fg = `theme.env_band_fg.<env>` — a new theme
  key that pairs a legible foreground with each `env_band.<env>` bg.
- Env name: **bold**, UPPERCASED, fg = `theme.env_band_fg.<env>`.
- Mode chip: inline `[RW]` (fg = `theme.mode.rw`) or `[RO]` (fg =
  `theme.mode.ro`). Brackets in `theme.muted_fg`, glyph color in mode
  color. The mode chip remains legible regardless of env-bg.
- Layout label: fg = `theme.muted_fg`. Conveys settings, not safety;
  staying muted reinforces hierarchy.

#### Truncation rules (top bar)

When the assembled label exceeds row width, drop fields in this order
(safety-first — the mode chip and env name are the most load-bearing
signals and should survive the longest):

1. layout label (`single` / `triplet` / `incident`)
2. env name (`UNKNOWN`)
3. context name (truncated to fit, ellipsis suffix in `muted_fg`)

The mode chip (`[RW]` / `[RO]`) is never dropped — it always renders,
even when the row collapses to just `│ <ctx…> │ [RW] │`.

### Dashboard summary header — chip grid

Replaces the two muted text lines (cluster identity + scale counts) at
the top of `render_summary`. CPU/MEM bars and the `pulse · N pins`
title-border remain unchanged.

```
 pulse · 13 pins ──────────────────────────────────────────────
 CONTEXT enginyyr-enginyyr-oidc   K8S v1.31.1   NODES 3/3 ready   NS 11
 PODS 40   DEPLOYS 16   SVCS 21
 CPU    3%  █░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  0.27 / 10 cores
 MEM   46%  ██████████████░░░░░░░░░░░░░░░░  8.46 GiB / 18.5 GiB
```

Two rows of k9s-style chips, label-then-value. Row 1 carries identity
(CONTEXT, K8S, NODES, NS). Row 2 carries scale (PODS, DEPLOYS, SVCS).
Two spaces between chips, no glyph noise.

#### Truncation rules

- Each chip computes its width: `label_w + 1 + value_w`.
- Chips render left-to-right within the band width. If the next chip
  would overflow, drop it and append a muted `…` after the last fitting
  chip.
- Truncate order is right-to-left within each row — leftmost chips
  (CONTEXT on row 1, PODS on row 2) have priority.

## Theme schema additions

All additive. Existing theme TOMLs continue to parse unchanged.

```toml
# Foreground paired with each env_band bg — so context name + env
# label remain legible on every env palette.
[env_band_fg]
prod    = "white"     # over prod red
staging = "black"     # over staging yellow
dev     = "black"     # over dev green
local   = "black"     # over local cyan
unknown = "white"     # over unknown darkgray

# RW / RO chip foreground in the top status bar.
[mode]
rw = "red"            # mutations allowed → hot
ro = "green"          # read-only → safe

# Dashboard chip-header coloring.
[chip]
label_fg = "cyan"     # "CONTEXT", "K8S", "NODES", ...
value_fg = "reset"    # values default to terminal fg (always legible)
```

Implementation pattern mirrors `StatusColors` / `GaugeColors`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct EnvBandFg {
    #[serde(default = "default_env_band_fg_prod")]
    pub prod: ThemeColor,
    // …
}

impl Default for EnvBandFg { /* delegates to default_* fns */ }
```

`ThemeColor::Named("reset")` already maps to `Color::Reset`, which the
terminal renders as the user's default foreground — guaranteed legible
on every background.

## Color-rule reference

Every color cites a theme key. No hex literals or hardcoded ratatui
colors anywhere in the render paths.

### Top status bar

| Element | Theme key |
|---|---|
| Row bg | `env_band.<env>` *(existing)* |
| Context name fg, bold | `env_band_fg.<env>` *(new)* |
| Env name fg, bold caps | `env_band_fg.<env>` *(new)* |
| Mode chip glyph fg | `mode.rw` or `mode.ro` *(new)* |
| Mode chip brackets fg | `muted_fg` *(existing)* |
| Layout label fg | `muted_fg` *(existing)* |
| Separator `│` fg | `muted_fg` *(existing)* |

### Dashboard chip header

| Element | Theme key |
|---|---|
| Chip label (`CONTEXT`, `K8S`, …), bold | `chip.label_fg` *(new)* |
| Chip value default, bold | `chip.value_fg` *(new)* |
| `NODES x/y ready` value, health-zoned | `gauge.ok` / `gauge.warn` / `gauge.danger` *(existing)* |
| `K8S vX.Y.Z` value | `chip.value_fg` *(new)* |
| Truncation ellipsis `…` | `muted_fg` *(existing)* |
| `pulse · N pins` band title | unchanged *(default)* |

`NODES x/y ready` zoning:
- `gauge.ok` when `x == y`
- `gauge.warn` when `0 < x < y`
- `gauge.danger` when `x == 0`

## Implementation surface

Two files touched. No new crates, no new dependencies.

### `app/crates/cruster-tui/src/theme.rs`

Add three new structs (`EnvBandFg`, `ModeColors`, `ChipColors`) and
their `Default` impls + `default_*` color fns. Wire them into `Theme`
as new `#[serde(default)]` fields. ~80 LoC.

### `app/crates/cruster-tui/src/app.rs::render_safety_badge`

Rewrite the single `Paragraph::new(label).style(...)` into a `Line`
composed of styled `Span`s. Pull bg from `theme.env_band.<env>`, fg
from `theme.env_band_fg.<env>`. Inline mode chip and separators as
their own spans. ~40 LoC.

### `app/crates/cruster-tui/src/views/dashboard.rs::render_summary`

Replace the construction of `line1` (cluster identity) and `line2`
(scale counts) with a call to a new helper:

```rust
fn build_header_chips(
    summary: &Summary,
    context_name: &str,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>>
```

Helper returns two `Line`s of styled `Span`s, applying truncation when
combined chip widths exceed `width`. Unit-testable in isolation. ~120
LoC including helper.

Net diff: ~240 LoC added, ~30 LoC removed.

## Testing

### `theme.rs`

- All bundled themes (`Theme::bundled_names()`) parse with the new
  fields and produce sensible defaults (no `Color::Reset` where a
  color is required, no missing fields after `serde(default)`).
- Custom TOML overriding `chip.label_fg = "magenta"` round-trips and
  the resolved color is `Color::Magenta`.

### `app.rs::render_safety_badge`

- `TestBackend` snapshot for each `Environment` variant (prod, staging,
  dev, local, unknown): assert that the row at `y=0` has the expected
  `env_band.<env>` bg cell-by-cell.
- For each variant: assert that the context-name cell's fg is the
  matching `env_band_fg.<env>` (proves the pairing is wired).
- Mode chip: with `read_only = true`, assert the `[RO]` glyphs use
  `theme.mode.ro`; with `read_only = false`, `[RW]` uses `theme.mode.rw`.
- Width regression: at width 40, the layout label is dropped before the
  mode chip (mode is a safety signal; layout is a setting).

### `views/dashboard.rs::build_header_chips`

- Pure helper: feed it a fixed `Summary` + a `Theme`, assert returned
  `Vec<Line>` length is 2 (identity row + scale row).
- Truncation: at width 30, trailing chips are dropped and the last
  fitting span carries the `…` suffix in `muted_fg`.
- Health zoning: assert `NODES 3/3 ready` resolves to `gauge.ok`,
  `NODES 1/3 ready` to `gauge.warn`, `NODES 0/3 ready` to
  `gauge.danger`. Three parameterized cases.
- Snapshot test rendering the full `render_summary` to a `TestBackend`,
  scanning the buffer for `CONTEXT`, `K8S`, `NODES`, `NS`, `PODS`,
  `DEPLOYS`, `SVCS` labels — proves the header survives integration.

## Open questions

None — design fully specified.

## Follow-ups (out of scope)

- The bundled non-`terminal` themes ship with the new keys' defaults;
  per-theme tuning (e.g., `monokai.toml` overrides `chip.label_fg` to
  a Monokai-pink) is a polish pass for a later phase.
- `DashboardView::new()` reads from the user's real `~/.config/cruster/
  dashboard.toml`, which makes tests environment-dependent (the
  `footer_renders_applicable_actions` test was already patched in this
  branch to dodge the issue). A proper fix — injecting the config path
  or a test-only constructor — is its own task.
