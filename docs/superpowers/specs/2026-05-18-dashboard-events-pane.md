# Dashboard events pane

**Date:** 2026-05-18
**Status:** Approved
**Owner:** Doug

## Summary

Add a fourth band to the pulse dashboard that surfaces recent
warnings + normal events cluster-wide, occupying the bottom ~1/4
of the dashboard area. Operational-alarm panel: "what's broken
right now, in one glance, without leaving the dashboard".

## Layout

Today (`views/dashboard.rs::render`):

```
+----------------------------------+
| Summary (6 rows, fixed)          |
+----------------------------------+
| Trends  (9 rows, fixed)          |
+----------------------------------+
| Pins    (Min 2, takes remainder) |
+----------------------------------+
```

After:

```
+----------------------------------+
| Summary (6 rows, fixed)          |
+----------------------------------+
| Trends  (9 rows, fixed)          |
+----------------------------------+
| Pins    (Min 2, takes remainder) |
+----------------------------------+
| Events  (max(4, total_h / 4))    |
+----------------------------------+
```

Pins gets squeezed first; the events band claims its 1/4 budget
deterministically. Min-floor of 4 rows so the band still renders
meaningfully on a very small terminal (header + 2-3 events).

## Source + filter

- Source: `registry.events.snapshot()` — same store the `:events`
  view consumes.
- Sort: Warnings first, then Normals, both by `lastTimestamp` desc.
- Cap: take the most-recent `available_event_rows` (height minus
  header).
- Empty: render a single muted "no events" placeholder.

## Visual

```
─ events · 3 warnings · 12 normal ─────────────────────────────────
 12s  ⚠  FailedScheduling  Pod/api-7d…    0/3 nodes available: 2 …
 45s  ⚠  BackOff           Pod/worker-2   Back-off restarting fail…
 1m   ⚠  Unhealthy         Pod/db-0       Readiness probe failed: …
 2m   ·  Pulled            Pod/api-7d…    Successfully pulled imag…
 3m   ·  Created           Deploy/api     Created pod: api-7d4f9…
```

- Columns: AGE (5), TYPE glyph (2), REASON (16), OBJECT (24),
  MESSAGE (rest, truncated with `…`).
- Warning rows tinted `theme.status.failed` via the existing
  `views::unhealthy_row_style` helper — the row-tint pattern that
  shipped in PR #36.
- Normal rows render in `theme.muted_fg`.
- Glyphs: `⚠` for Warning, `·` for Normal.
- Title chip: `" events · N warnings · M normal "`.
- Top border only (`Borders::TOP`) — matches the pins band's
  borderless look.

## Interactivity

None in v1. The pane is read-only, like the trends band. Pin
selection (`j/k/x/Enter`) keeps its existing semantics. `:events`
view stays as the full interactive surface.

Why: keeping the dashboard's focus model simple means we don't
have to introduce a focus indicator or another Tab-cycle target.
If users ask for it, jumping straight to the event's involved-
object view (Enter on a row) is the obvious follow-up.

## Implementation sketch

- **`views/dashboard.rs::render`** — add a 4th `Constraint` to
  the vertical Layout split. Compute the events constraint as
  `Length(max(4, area.height / 4))` BEFORE the `Min(2)` pins
  constraint so pins shrinks first.
- **`views/dashboard.rs::render_events`** (new) — pure render
  function over `&self`, takes the band's `Rect`, the events
  snapshot, and the theme. Builds a single-column `Table` (or
  raw `Paragraph` lines if Table chrome is too heavy).
- **`DashboardView::events`** field — `Vec<(ResourceKey, Event)>`
  cached on `refresh()` from `registry.events.snapshot()`. Keep
  it sorted at refresh time so render is O(n) cell-build.
- **`refresh`** — extend to pull events alongside the existing
  metrics/pod-stats fetches.
- **No new theme keys** — reuses `status.failed`, `muted_fg`,
  `header_fg`.

## Test plan

- `events_band_height_floor_is_4_in_tiny_areas` — split arithmetic
  when total height is small.
- `events_band_takes_quarter_of_total_height` — split arithmetic.
- `render_warning_row_uses_status_failed_fg` — TestBackend cell
  assertion on a Warning row's MESSAGE column.
- `render_normal_row_uses_muted_fg` — TestBackend cell assertion.
- `warnings_sort_before_normals_with_same_recency` — pure-logic
  test on the sort step.
- `render_empty_state_shows_no_events_placeholder` — TestBackend
  search for the placeholder text.

## Acceptance criteria

- [ ] Dashboard shows a 4-band layout; events band occupies the
  bottom `max(4, h/4)` rows.
- [ ] Warning events render with the `status.failed` row tint;
  Normal events render in `muted_fg`.
- [ ] Sort order: Warnings first, both groups newest-first.
- [ ] Empty event store → "no events" placeholder, no crash.
- [ ] Pin selection (`j/k/x/Enter`) keeps working unchanged.
- [ ] `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo test --workspace` all clean.
- [ ] PR description includes a smoke recipe.
- [ ] No merge until the user confirms the band looks right live.

## Per-task TDD breakdown

1. Add `events` field + extend `refresh()` to populate it sorted.
2. Add `render_events` + wire it into the 4-band layout.
3. Empty state + Warning/Normal styling tests.
4. Push branch, open PR with smoke recipe, wait for user
   go-ahead, merge.

## Out of scope (follow-ups)

- Per-object filtering (events for pinned resources only).
- Click/Enter to jump to the event's involved-object view.
- Persistent dismissal of acknowledged warnings.
- Sound / desktop notification on new Warning.
