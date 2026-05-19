# Color UX: zero-ready cells + overlay borders

**Date:** 2026-05-18
**Status:** Approved
**Owner:** Doug

## Summary

Two small visual treatments that lift the default theme out of "drab":

1. List rows where `READY = 0/N` get the READY cell painted in
   `theme.status.failed` (bold red). The rest of the row stays
   default-styled so the selection highlight remains readable.
2. Every modal overlay (delete, help, port-forward, palette,
   relationships, search) gets a colored border using a new theme
   slot `overlay_border`, defaulting to bright cyan in the
   terminal theme.

Both ship as defaults in the `terminal` theme — no opt-in required.

## Non-goals

- Recoloring entire rows by health (red row for failing pod). Cell-
  level cue is enough; row-level highlight is reserved for selection.
- Per-modal semantic border colors (separate red/blue/cyan per
  modal type). User chose "uniform accent" — keep it simple.
- Repainting partial-ready rows (e.g. `1/3`) in amber. v1 is just
  the `0/N` red treatment; partial states stay default. Easy to
  revisit if it reads too binary.
- Coverage for StatefulSets/DaemonSets/Jobs READY columns. Those
  views haven't shipped yet (tickets #23, #24). When they do, they
  inherit the same helper.
- Per-bundled-theme tuning of the new `overlay_border` slot. The
  8 non-terminal themes accept the default via `#[serde(default)]`
  and can be polished in a follow-up.

## Visual targets

### Zero-ready READY cell

```
NAMESPACE  NAME    STATUS    READY   RESTARTS
default    nginx   Running   1/1     0
default    api     Pending   0/3     5         ← READY in bold red
```

### Overlay border (delete modal, others identical except no red title)

```
┌─ Delete Pod/nginx in default? ────────────┐  ← border in overlay_border (cyan)
│                                            │     title in status.failed (red, bold)
│  Propagation:  [ Background ]  …           │
│  …                                         │
└────────────────────────────────────────────┘
```

## Architecture

### A. Zero-ready cell

A pure helper in `cruster-tui` (or inline per view) that, given a
"X/Y" READY string, returns either:

- `(text, Style::default())` for `X > 0`
- `(text, theme.status.failed style + BOLD)` for `X == 0` (and Y > 0)

Applied at the call site that builds the `Cell` in the table row.
Tested via `TestBackend` cell-color assertion.

### B. Overlay border

New `Theme` field:

```rust
#[serde(default = "default_overlay_border")]
pub overlay_border: ThemeColor,

fn default_overlay_border() -> ThemeColor { ThemeColor::indexed_cyan() }
```

Every overlay's `Block::default().borders(Borders::ALL)` gains a
`.border_style(Style::default().fg(theme.overlay_border.as_ratatui()))`.

Some overlays (palette, relationships) build multiple blocks; each
gets the same border style. The delete modal keeps its existing red
title styling — that's `title_style`, not the border.

### C. Theme threading

Search + palette overlays don't currently receive a theme. They
default to `Theme::terminal_default()` inside their render method
(matches the pattern delete + help use). No change to overlay
signatures or App wiring.

## Test plan

- **Pods view:**
  - `ready_cell_styled_failed_when_zero_containers_ready` —
    fixture pod with 0 ready containers; assert the READY cell's
    style fg matches `theme.status.failed`.
  - `ready_cell_not_styled_when_partial` (1/3 → default fg).
  - `ready_cell_not_styled_when_fully_ready` (3/3 → default fg).
- **Deployments view:** mirror the three above with replica counts.
- **Overlays:** one test per overlay
  (`delete`, `help`, `port_forward`, `palette`, `relationships`,
  `search`) that renders into a `TestBackend` and asserts the
  top-left border corner cell uses `theme.overlay_border.as_ratatui()`.
- **Theme:** `terminal_default` exposes a non-default
  `overlay_border` value (bright cyan); `#[serde(default)]` test
  proves TOMLs without the key still parse.

## Acceptance criteria

- [ ] Pods view shows `0/N` rows with the READY cell in bold red;
  `1/N` and `N/N` unchanged.
- [ ] Deployments view shows the same treatment.
- [ ] All 6 overlays render with a colored border using
  `theme.overlay_border`; default theme ships a visible cyan.
- [ ] No existing tests regress.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo test --workspace` all clean.
- [ ] PR description includes a one-liner smoke recipe so the user
  can verify the colors live before merge (per the merge-gating
  feedback).

## Per-task TDD breakdown

1. Add `overlay_border` slot + default + theme test.
2. Pods view: TDD'd zero-ready cell coloring.
3. Deployments view: TDD'd zero-ready cell coloring.
4. All 6 overlays: TDD'd border-style application.
5. Push branch, post PR with smoke recipe, wait for user go-ahead,
   merge.

## Out of scope (follow-ups)

- Partial-ready amber treatment.
- Health-derived row backgrounds.
- Per-bundled-theme tuning of `overlay_border`.
- Same treatment for STS / DS / Jobs / CronJobs once those views land.
