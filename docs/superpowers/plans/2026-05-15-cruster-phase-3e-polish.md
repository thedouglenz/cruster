# Cruster Phase 3E: Polish — De-k9s + Pane Focus + Modal Port-Forward

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Three UX critiques surfaced during live testing:

1. **Looks too much like k9s.** Heavy borders, full-row reverse-video
   selection, verbose titles ("— j/k move · :kind switch · q quit")
   are k9s's visual signature. Cruster needs to look like something
   newer.
2. **Pane navigation is immature.** Pressing `d` opens describe; the
   pane then swallows every key until Esc. There's no way to cycle
   focus between the list and the pane.
3. **Port-forward prompt should be modal.** The current bottom-bar
   input is easy to miss and clashes visually with toast/command-mode.
   A centered modal (palette-style) reads better.

This is the final polish sub-phase of Phase 3 before Phase 4. After
this lands, the TUI should feel meaningfully different from k9s.

**Architecture:**
- Per-kind view rendering moves away from `Borders::ALL` to a sparse
  header line + columns. Selected row gets a left-edge accent
  (`▎` or similar) plus a foreground color shift, not a full reverse.
- `App` gains a `PaneFocus` field (`View` | `Describe` | `Logs`).
  `Tab` cycles through valid focus targets. When focus is on a pane,
  keys route there; otherwise to the view.
- `PortForwardPrompt` becomes a proper `Overlay` (centered modal).
  The bottom-bar prompt code goes away.

---

## Task 1: Visual de-k9s — strip table borders + new selection style

**Files modified:**
- All 8 view files (`pods.rs`, `deployments.rs`, …, `namespaces.rs`)

The same pattern in every view. For each:
- Replace `Block::default().borders(Borders::ALL).title(...)` with
  `Block::default().borders(Borders::TOP).title(" pods · 10 ")` —
  just a thin top rule with a compact title.
- Drop the long "— j/k move · :kind switch · q quit" tail from the
  title (those are in the action footer now).
- Change selection style from `Style::default().add_modifier(Modifier::REVERSED)`
  to `Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)`
  + a left-edge accent column in the table (we use a fixed
  `Constraint::Length(2)` leftmost column that's "▎" only on the
  selected row).

For Phase 3E v1, the accent column is a simple approach: add a
narrow leftmost column that renders "▎" for the selected row and
" " otherwise. Cleaner-looking than full-row reverse.

- [ ] **Step 1: Update PodsView (canonical)**

In `pods.rs`, in the `render` method:

```rust
fn render(&self, frame: &mut Frame<'_>) {
    let area = frame.area();

    let header = Row::new(vec!["", "NAMESPACE", "NAME", "STATUS", "READY", "RESTARTS"])
        .style(Style::default().fg(Color::DarkGray));

    let table_rows: Vec<Row> = self
        .snapshot
        .iter()
        .enumerate()
        .map(|(i, (key, pod))| {
            let ns = key.namespace.as_deref().unwrap_or("-");
            let marker = if i == self.selected { "▎" } else { " " };
            let row = Row::new(vec![
                Cell::from(marker),
                Cell::from(ns.to_string()),
                Cell::from(key.name.clone()),
                Cell::from(pod_phase(pod)),
                Cell::from(pod_ready(pod)),
                Cell::from(pod_restarts(pod).to_string()),
            ]);
            if i == self.selected {
                row.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(2),
        Constraint::Length(20),
        Constraint::Min(20),
        Constraint::Length(14),
        Constraint::Length(8),
        Constraint::Length(10),
    ];

    let table = Table::new(table_rows, widths).header(header).block(
        Block::default()
            .borders(Borders::TOP)
            .title(format!(" pods · {} ", self.snapshot.len())),
    );

    frame.render_widget(table, area);
}
```

(Note: no more `TableState` / `row_highlight_style` since we paint
the selected row's style directly in the row construction. Drop the
`use ratatui::widgets::TableState;` import if it's unused.)

Add `Color` to the existing ratatui style imports.

- [ ] **Step 2: Apply the same pattern to the other 7 views**

Each view's render gets:
- A leftmost column for the `▎` marker
- Title shortened to `" <kind> · N "`
- `Borders::TOP` only
- Direct selected-row styling

The pattern is mechanical. Each view follows the structure above.

- [ ] **Step 3: Test + clippy + commit**

```bash
cd app && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git add app
git commit -m "feat(tui): drop heavy borders, switch to left-edge selection accent"
```

---

## Task 2: Pane focus model — Tab cycles between view and open pane

**Files modified:**
- `app/crates/cruster-tui/src/app.rs`

- [ ] **Step 1: Add PaneFocus enum**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneFocus {
    View,
    Describe,
    Logs,
}
```

Add `pane_focus: PaneFocus` field to App, default `PaneFocus::View`.

- [ ] **Step 2: Tab cycles**

In `handle_key`, before the keymap dispatch, handle `KeyCode::Tab`:

```rust
if key.code == KeyCode::Tab {
    self.cycle_pane_focus();
    return LoopState::Continue;
}
```

```rust
fn cycle_pane_focus(&mut self) {
    let candidates = self.focus_candidates();
    if candidates.len() < 2 {
        return;
    }
    let current_idx = candidates.iter().position(|f| *f == self.pane_focus).unwrap_or(0);
    self.pane_focus = candidates[(current_idx + 1) % candidates.len()];
}

fn focus_candidates(&self) -> Vec<PaneFocus> {
    let mut v = vec![PaneFocus::View];
    if self.describe_pane.is_open() {
        v.push(PaneFocus::Describe);
    }
    if self.logs_pane.is_open() {
        v.push(PaneFocus::Logs);
    }
    v
}
```

- [ ] **Step 3: Route keys to focused pane**

Currently `handle_key` has hard checks: "if describe_pane.is_open(),
route there." Change so the pane-routing only happens when
`pane_focus == Describe/Logs`:

```rust
match self.pane_focus {
    PaneFocus::Describe if self.describe_pane.is_open() => {
        self.describe_pane.handle_key(key);
        return LoopState::Continue;
    }
    PaneFocus::Logs if self.logs_pane.is_open() => {
        self.logs_pane.handle_key(key);
        return LoopState::Continue;
    }
    _ => {}
}
```

When focus is `View`, keys go to the view even if a pane is open.
That lets the user `j`/`k` through the list while the describe
pane stays visible (auto-following selection happens in Phase 4
via the relationship-first nav; for 3E v1 the pane shows the
content from when it was opened).

- [ ] **Step 4: When opening a pane, focus it; when closing, focus the view**

In the describe/logs handlers, after opening:
```rust
self.pane_focus = PaneFocus::Describe;
```

In the pane key handlers, when Esc closes it, the App's main loop
detects the pane is no longer open and resets focus next tick — but
simpler: detect closure in handle_key and reset focus immediately.

Actually cleanest: at the top of `handle_key`, normalise pane_focus:
```rust
if matches!(self.pane_focus, PaneFocus::Describe) && !self.describe_pane.is_open() {
    self.pane_focus = PaneFocus::View;
}
if matches!(self.pane_focus, PaneFocus::Logs) && !self.logs_pane.is_open() {
    self.pane_focus = PaneFocus::View;
}
```

- [ ] **Step 5: Visual focus indicator**

In each pane's render, add `(focused)` to the title if focused. The
App passes a `focused: bool` arg to the pane render methods. Update
`describe.rs::DescribePane::render` and `logs.rs::LogsPane::render`
to take `&self, frame, area, focused: bool` and reflect in title.

- [ ] **Step 6: Test + commit**

```bash
git add app
git commit -m "feat(tui): pane focus model with Tab to cycle, visual focus indicator"
```

---

## Task 3: Port-forward as a modal overlay

**Files:**
- Create: `app/crates/cruster-tui/src/overlays/port_forward.rs`
- Modify: `app/crates/cruster-tui/src/overlays/mod.rs`
- Modify: `app/crates/cruster-tui/src/app.rs`

Replace the bottom-bar `PortForwardPrompt` with a centered modal
overlay. Same input semantics, better visibility.

- [ ] **Step 1: Overlay**

```rust
//! Port-forward modal: prompts the user for `local:remote` mapping.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use cruster_core::ResourceKey;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::overlay::{Overlay, OverlayResult};

pub struct PortForwardOverlay {
    pub pod_key: ResourceKey,
    buffer: String,
}

impl PortForwardOverlay {
    pub fn new(pod_key: ResourceKey) -> Self {
        Self {
            pod_key,
            buffer: String::new(),
        }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }
}

impl Overlay for PortForwardOverlay {
    fn handle_key(&mut self, key: KeyEvent) -> OverlayResult {
        if key.kind != KeyEventKind::Press {
            return OverlayResult::KeepOpen;
        }
        match key.code {
            KeyCode::Esc => OverlayResult::Close,
            KeyCode::Enter => {
                // Custom: ask App to dispatch by id "port-forward-submit"
                // which expects the buffer in a side-channel. The
                // app reads our buffer before processing.
                OverlayResult::Invoke("port-forward-submit".into())
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                OverlayResult::KeepOpen
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                OverlayResult::KeepOpen
            }
            _ => OverlayResult::KeepOpen,
        }
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let w = area.width.saturating_sub(20).min(60);
        let h = 5;
        if w < 20 || h > area.height {
            return;
        }
        let x = area.x + (area.width - w) / 2;
        let y = area.y + (area.height - h) / 2;
        let rect = Rect { x, y, width: w, height: h };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(2)])
            .split(rect);

        let title = format!(" port-forward pod/{} ", self.pod_key.name);
        let input = Paragraph::new(format!("local:remote → {}_", self.buffer))
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().bg(Color::Reset));
        frame.render_widget(input, chunks[0]);

        let hint = Paragraph::new("enter applies · esc cancels")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[1]);
    }
}
```

- [ ] **Step 2: App wiring — replace bottom-bar prompt**

- Remove `port_forward_prompt: Option<PortForwardPrompt>` field +
  `start_port_forward_prompt` + `handle_port_forward_prompt_key` +
  the overlay rendering for it.
- In `start_port_forward_prompt`, set
  `self.overlay = Some(Box::new(PortForwardOverlay::new(key)))`.
- In the OverlayResult::Invoke branch, before dispatching by id,
  check for `"port-forward-submit"`: if so, downcast the overlay
  to PortForwardOverlay (via a new trait method
  `port_forward_buffer() -> Option<(ResourceKey, &str)>`) and call
  `PortForward::start(...)`.
- Drop `PortForwardPrompt` struct.

- [ ] **Step 3: Test + commit**

```bash
git add app
git commit -m "feat(tui): replace bottom-bar port-forward prompt with modal overlay"
```

---

## Task 4: Phase 3E exit verification

- [ ] fmt + clippy + test
- [ ] Manual k3d:
  - Top bar shows the safety badge in colour, view below is borderless
    except for a thin top line
  - Selected row has `▎` left-edge accent + bold cyan text (no full
    reverse)
  - `d` opens describe pane (focus = Describe). Tab returns focus to
    view; `j`/`k` move the view's selection while describe stays
    visible. Tab again → describe; Esc closes.
  - `f` on a pod → centered modal asking for `local:remote`. Enter
    applies; Esc cancels.
- [ ] Update READMEs
- [ ] Tag `phase-3e-polish`
- [ ] Build release, push to pane 2 for user to verify

---

## Phase 3E exit criteria

1. fmt + clippy + test clean.
2. Visual: no full-row reverse highlights; minimal borders; concise titles.
3. Pane focus: Tab cycles; focus shows in pane title; view keys work while pane is open.
4. Port-forward is modal (not bottom bar).
5. User confirmation that it feels less k9s-y.
