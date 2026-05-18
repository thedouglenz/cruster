# Cruster Phase 6B: Dashboard header & status-bar color treatment

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring k9s-grade color and information hierarchy to the cruster
TUI's top status bar and the dashboard summary band, by upgrading
foreground colors and replacing the muted summary text with a chip-style
header — entirely theme-parameterized via three new additive theme
structs.

**Architecture:** Three small additions to `theme.rs` (`EnvBandFg`,
`ModeColors`, `ChipColors`) provide the new color slots. The top
status bar in `app.rs::render_safety_badge` is rebuilt as a `Line` of
styled `Span`s using the new keys. The summary band in
`views/dashboard.rs::render_summary` delegates its identity + scale
lines to a new pure helper `build_header_chips` that returns
truncation-aware `Vec<Line<'static>>`.

**Tech Stack:** Rust, ratatui (Style/Span/Line/Paragraph), serde
(theme TOML), TestBackend for snapshot assertions.

**Spec:** `docs/superpowers/specs/2026-05-17-cruster-phase-6b-dashboard-header-color.md`

---

## File map

- **Modify** `app/crates/cruster-tui/src/theme.rs` — add 3 structs
  (`EnvBandFg`, `ModeColors`, `ChipColors`) + their `Default` impls +
  `default_*` color fns, wired into `Theme` as new `#[serde(default)]`
  fields.
- **Modify** `app/crates/cruster-tui/src/app.rs::render_safety_badge`
  — rewrite to use themed `Span`s, add width-aware truncation.
- **Modify** `app/crates/cruster-tui/src/views/dashboard.rs` — add
  `build_header_chips(...) -> Vec<Line<'static>>`, swap into
  `render_summary` in place of `line1`/`line2`.

No new files, no new dependencies. Existing theme TOMLs continue to
parse unchanged.

---

## Task 1: Add `EnvBandFg` theme struct

**Files:**
- Modify: `app/crates/cruster-tui/src/theme.rs`

- [ ] **Step 1: Write the failing test**

Append to the existing `mod tests` block in `theme.rs` (after the
`named_colors_map_correctly` test):

```rust
    #[test]
    fn env_band_fg_defaults_pair_with_env_band_bgs() {
        let t = Theme::terminal_default();
        assert_eq!(t.env_band_fg.prod.as_ratatui(), Color::White);
        assert_eq!(t.env_band_fg.staging.as_ratatui(), Color::Black);
        assert_eq!(t.env_band_fg.dev.as_ratatui(), Color::Black);
        assert_eq!(t.env_band_fg.local.as_ratatui(), Color::Black);
        assert_eq!(t.env_band_fg.unknown.as_ratatui(), Color::White);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::tests::env_band_fg_defaults_pair_with_env_band_bgs
```
Expected: `error[E0609]: no field 'env_band_fg' on type 'Theme'`

- [ ] **Step 3: Add `EnvBandFg` struct, defaults, and wire into `Theme`**

In `theme.rs`, add the struct (place it alongside `EnvBand`):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct EnvBandFg {
    #[serde(default = "default_env_band_fg_prod")]
    pub prod: ThemeColor,
    #[serde(default = "default_env_band_fg_staging")]
    pub staging: ThemeColor,
    #[serde(default = "default_env_band_fg_dev")]
    pub dev: ThemeColor,
    #[serde(default = "default_env_band_fg_local")]
    pub local: ThemeColor,
    #[serde(default = "default_env_band_fg_unknown")]
    pub unknown: ThemeColor,
}

impl Default for EnvBandFg {
    fn default() -> Self {
        Self {
            prod: default_env_band_fg_prod(),
            staging: default_env_band_fg_staging(),
            dev: default_env_band_fg_dev(),
            local: default_env_band_fg_local(),
            unknown: default_env_band_fg_unknown(),
        }
    }
}
```

Add the default fns (place them next to the existing `default_prod` /
`default_staging` block):

```rust
fn default_env_band_fg_prod() -> ThemeColor {
    ThemeColor::Named("white".into())
}
fn default_env_band_fg_staging() -> ThemeColor {
    ThemeColor::Named("black".into())
}
fn default_env_band_fg_dev() -> ThemeColor {
    ThemeColor::Named("black".into())
}
fn default_env_band_fg_local() -> ThemeColor {
    ThemeColor::Named("black".into())
}
fn default_env_band_fg_unknown() -> ThemeColor {
    ThemeColor::Named("white".into())
}
```

Wire into `Theme` by inserting this field next to `pub env_band`:

```rust
    #[serde(default)]
    pub env_band_fg: EnvBandFg,
```

- [ ] **Step 4: Run test to verify it passes**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::tests::env_band_fg_defaults_pair_with_env_band_bgs
```
Expected: `test result: ok. 1 passed`

Also run the full theme test module to confirm nothing else broke:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::
```
Expected: all theme tests pass.

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/theme.rs
git commit -m "feat(theme): add env_band_fg paired with env_band bgs"
```

---

## Task 2: Add `ModeColors` theme struct

**Files:**
- Modify: `app/crates/cruster-tui/src/theme.rs`

- [ ] **Step 1: Write the failing test**

Append to the same `mod tests` block:

```rust
    #[test]
    fn mode_colors_defaults() {
        let t = Theme::terminal_default();
        assert_eq!(t.mode.rw.as_ratatui(), Color::Red);
        assert_eq!(t.mode.ro.as_ratatui(), Color::Green);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::tests::mode_colors_defaults
```
Expected: `error[E0609]: no field 'mode' on type 'Theme'`

- [ ] **Step 3: Add `ModeColors` struct, defaults, and wire into `Theme`**

In `theme.rs`, add the struct (place it alongside `EnvBand` /
`EnvBandFg`):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ModeColors {
    #[serde(default = "default_mode_rw")]
    pub rw: ThemeColor,
    #[serde(default = "default_mode_ro")]
    pub ro: ThemeColor,
}

impl Default for ModeColors {
    fn default() -> Self {
        Self {
            rw: default_mode_rw(),
            ro: default_mode_ro(),
        }
    }
}
```

Add the default fns:

```rust
fn default_mode_rw() -> ThemeColor {
    ThemeColor::Named("red".into())
}
fn default_mode_ro() -> ThemeColor {
    ThemeColor::Named("green".into())
}
```

Wire into `Theme`:

```rust
    #[serde(default)]
    pub mode: ModeColors,
```

- [ ] **Step 4: Run test to verify it passes**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::tests::mode_colors_defaults
```
Expected: `test result: ok. 1 passed`

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/theme.rs
git commit -m "feat(theme): add mode colors for rw/ro chip in status bar"
```

---

## Task 3: Add `ChipColors` theme struct

**Files:**
- Modify: `app/crates/cruster-tui/src/theme.rs`

- [ ] **Step 1: Write the failing test**

Append to `mod tests`:

```rust
    #[test]
    fn chip_colors_defaults() {
        let t = Theme::terminal_default();
        assert_eq!(t.chip.label_fg.as_ratatui(), Color::Cyan);
        // "reset" maps to Color::Reset — terminal's default fg.
        assert_eq!(t.chip.value_fg.as_ratatui(), Color::Reset);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::tests::chip_colors_defaults
```
Expected: `error[E0609]: no field 'chip' on type 'Theme'`

- [ ] **Step 3: Add `ChipColors` struct, defaults, and wire into `Theme`**

In `theme.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ChipColors {
    #[serde(default = "default_chip_label_fg")]
    pub label_fg: ThemeColor,
    #[serde(default = "default_chip_value_fg")]
    pub value_fg: ThemeColor,
}

impl Default for ChipColors {
    fn default() -> Self {
        Self {
            label_fg: default_chip_label_fg(),
            value_fg: default_chip_value_fg(),
        }
    }
}
```

Default fns:

```rust
fn default_chip_label_fg() -> ThemeColor {
    ThemeColor::Named("cyan".into())
}
fn default_chip_value_fg() -> ThemeColor {
    ThemeColor::Named("reset".into())
}
```

Wire into `Theme`:

```rust
    #[serde(default)]
    pub chip: ChipColors,
```

- [ ] **Step 4: Run test to verify it passes**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib theme::
```
Expected: all theme tests pass, including `chip_colors_defaults` and
`all_bundled_themes_parse` (the bundled themes pick up the new field's
defaults automatically).

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/theme.rs
git commit -m "feat(theme): add chip colors for dashboard header"
```

---

## Task 4: Rewrite `render_safety_badge` with themed Spans

**Files:**
- Modify: `app/crates/cruster-tui/src/app.rs`

This task replaces the single-`Paragraph` implementation with a `Line`
of styled `Span`s that source every color from the theme. Truncation
comes in Task 5.

- [ ] **Step 1: Write the failing test**

Append to the existing `mod tests` block in `app.rs` (after
`footer_renders_applicable_actions`):

```rust
    #[test]
    fn safety_badge_uses_env_band_fg_for_context_name() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use cruster_core::Environment;

        let mut a = app();
        a.context = "my-cluster".into();
        a.environment = Environment::Unknown;
        a.read_only = false;

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| a.render_safety_badge(f))
            .unwrap();
        let buf = terminal.backend().buffer();

        // Find the first cell on row 0 whose symbol is 'm' (start of
        // "my-cluster"). Its fg must be env_band_fg.unknown (white)
        // and bg must be env_band.unknown (darkgray).
        let want_bg = a.theme.env_band.unknown.as_ratatui();
        let want_fg = a.theme.env_band_fg.unknown.as_ratatui();
        let mut found = false;
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            if cell.symbol() == "m" {
                assert_eq!(cell.style().bg, Some(want_bg), "ctx bg");
                assert_eq!(cell.style().fg, Some(want_fg), "ctx fg");
                found = true;
                break;
            }
        }
        assert!(found, "expected to find the 'm' of 'my-cluster' on row 0");
    }

    #[test]
    fn safety_badge_mode_chip_uses_mode_rw_color_when_writable() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use cruster_core::Environment;

        let mut a = app();
        a.context = "ctx".into();
        a.environment = Environment::Unknown;
        a.read_only = false;

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let want = a.theme.mode.rw.as_ratatui();
        // The 'R' in "[RW]" should be painted with mode.rw.
        let mut found = false;
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            if cell.symbol() == "R" {
                assert_eq!(cell.style().fg, Some(want), "RW glyph fg");
                found = true;
                break;
            }
        }
        assert!(found, "expected to find 'R' (from [RW]) on row 0");
    }

    #[test]
    fn safety_badge_mode_chip_uses_mode_ro_color_when_read_only() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use cruster_core::Environment;

        let mut a = app();
        a.context = "ctx".into();
        // Staging chosen because "STAGING" has no 'R' character, so
        // the first 'R' we find on the row must come from "[RO]".
        // (Prod env label is "PROD" — would contain an 'R' too.)
        a.environment = Environment::Staging;
        a.read_only = true;

        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let want = a.theme.mode.ro.as_ratatui();
        let mut found = false;
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            if cell.symbol() == "R" {
                assert_eq!(cell.style().fg, Some(want), "RO glyph fg");
                found = true;
                break;
            }
        }
        assert!(found, "expected to find 'R' (from [RO]) on row 0");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib app::tests::safety_badge
```
Expected: 3 tests fail. The context cell will fail with `Some(Color::Black)` vs `Some(Color::White)` (existing code hardcodes black). The mode-chip tests will fail because there's no styled 'R' or 'O' yet (current text contains `rw`/`ro` lowercase).

- [ ] **Step 3: Rewrite `render_safety_badge`**

Replace the existing fn body (around `app.rs:984-1009`) with:

```rust
    fn render_safety_badge(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.height == 0 {
            return;
        }
        let env = self.environment;
        let bg = match env {
            Environment::Prod => self.theme.env_band.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band.dev.as_ratatui(),
            Environment::Local => self.theme.env_band.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band.unknown.as_ratatui(),
        };
        let env_fg = match env {
            Environment::Prod => self.theme.env_band_fg.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band_fg.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band_fg.dev.as_ratatui(),
            Environment::Local => self.theme.env_band_fg.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band_fg.unknown.as_ratatui(),
        };
        let muted_fg = self.theme.muted_fg.as_ratatui();
        let mode_color = if self.read_only {
            self.theme.mode.ro.as_ratatui()
        } else {
            self.theme.mode.rw.as_ratatui()
        };
        let mode_text = if self.read_only { "RO" } else { "RW" };
        let env_text = self.environment.to_string().to_uppercase();
        let layout_text = self.layout.label().to_string();

        let sep = Span::styled(" │ ", Style::default().bg(bg).fg(muted_fg));
        let value_style = Style::default()
            .bg(bg)
            .fg(env_fg)
            .add_modifier(Modifier::BOLD);
        let bracket_style = Style::default().bg(bg).fg(muted_fg);
        let mode_style = Style::default()
            .bg(bg)
            .fg(mode_color)
            .add_modifier(Modifier::BOLD);
        let muted_on_bg = Style::default().bg(bg).fg(muted_fg);

        let spans = vec![
            sep.clone(),
            Span::styled(self.context.clone(), value_style),
            sep.clone(),
            Span::styled(env_text, value_style),
            sep.clone(),
            Span::styled("[", bracket_style),
            Span::styled(mode_text.to_string(), mode_style),
            Span::styled("]", bracket_style),
            sep.clone(),
            Span::styled(layout_text, muted_on_bg),
            sep,
        ];

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().bg(bg));
        let rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }
```

You will need these imports at the top of `app.rs` (most already
present; add `Line`, `Span`, `Modifier` if missing):

```rust
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
```

Check with:
```
grep -n "^use ratatui::" app/crates/cruster-tui/src/app.rs
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib app::tests::safety_badge
```
Expected: all 3 `safety_badge_*` tests pass.

Then run the full cruster-tui suite to confirm nothing regressed:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib
```
Expected: `test result: ok. 147 passed` (144 baseline + 3 new tests).

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/app.rs
git commit -m "feat(tui): themed top status bar with span-level color and rw/ro chip"
```

---

## Task 5: Add top-bar truncation rules

**Files:**
- Modify: `app/crates/cruster-tui/src/app.rs`

When the assembled label exceeds row width, drop fields in this order:
layout label → env name → context name (truncated with `…`). The mode
chip is never dropped.

- [ ] **Step 1: Write the failing test**

Append to `mod tests`:

```rust
    #[test]
    fn safety_badge_drops_layout_first_when_narrow() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use cruster_core::Environment;

        let mut a = app();
        a.context = "short-ctx".into();
        a.environment = Environment::Prod;
        a.read_only = false;
        // Width chosen so env name fits but layout label does not.
        // Budget: reserved(13) + ctx(9) + sep+env(3+4) = 29.
        // Layout would add sep+layout(3+6) = 9 more -> needs width 38+.
        let backend = TestBackend::new(30, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let mut row0 = String::new();
        for x in 0..buf.area().width {
            row0.push_str(buf[(x, 0)].symbol());
        }
        // Mode chip must survive.
        assert!(row0.contains("[RW]"), "row should keep mode chip: {row0:?}");
        // Layout label "single" must be dropped at this width.
        assert!(
            !row0.contains("single"),
            "row should not contain layout label: {row0:?}"
        );
    }

    #[test]
    fn safety_badge_truncates_context_when_extremely_narrow() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use cruster_core::Environment;

        let mut a = app();
        a.context = "a-very-long-context-name-that-cannot-fit".into();
        a.environment = Environment::Unknown;
        a.read_only = false;
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| a.render_safety_badge(f)).unwrap();
        let buf = terminal.backend().buffer();

        let mut row0 = String::new();
        for x in 0..buf.area().width {
            row0.push_str(buf[(x, 0)].symbol());
        }
        // Mode chip must survive.
        assert!(row0.contains("[RW]"), "mode chip must survive: {row0:?}");
        // Context should be truncated with an ellipsis.
        assert!(row0.contains('…'), "context should be truncated: {row0:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib app::tests::safety_badge_drops_layout_first_when_narrow app::tests::safety_badge_truncates_context_when_extremely_narrow
```
Expected: both fail (Task 4 implementation always renders all fields).

- [ ] **Step 3: Implement truncation**

Replace the body of `render_safety_badge` to compute widths and assemble
spans conditionally. The full new body:

```rust
    fn render_safety_badge(&self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.height == 0 || area.width == 0 {
            return;
        }
        let env = self.environment;
        let bg = match env {
            Environment::Prod => self.theme.env_band.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band.dev.as_ratatui(),
            Environment::Local => self.theme.env_band.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band.unknown.as_ratatui(),
        };
        let env_fg = match env {
            Environment::Prod => self.theme.env_band_fg.prod.as_ratatui(),
            Environment::Staging => self.theme.env_band_fg.staging.as_ratatui(),
            Environment::Dev => self.theme.env_band_fg.dev.as_ratatui(),
            Environment::Local => self.theme.env_band_fg.local.as_ratatui(),
            Environment::Unknown => self.theme.env_band_fg.unknown.as_ratatui(),
        };
        let muted_fg = self.theme.muted_fg.as_ratatui();
        let mode_color = if self.read_only {
            self.theme.mode.ro.as_ratatui()
        } else {
            self.theme.mode.rw.as_ratatui()
        };
        let mode_text = if self.read_only { "RO" } else { "RW" };
        let env_text = self.environment.to_string().to_uppercase();
        let layout_text = self.layout.label().to_string();

        let sep_style = Style::default().bg(bg).fg(muted_fg);
        let value_style = Style::default()
            .bg(bg)
            .fg(env_fg)
            .add_modifier(Modifier::BOLD);
        let bracket_style = Style::default().bg(bg).fg(muted_fg);
        let mode_style = Style::default()
            .bg(bg)
            .fg(mode_color)
            .add_modifier(Modifier::BOLD);
        let muted_on_bg = Style::default().bg(bg).fg(muted_fg);

        // Mandatory fields (mode chip, separators around it).
        // Width budget: " │ <ctx> │ <ENV> │ [RW] │ <layout> │"
        // Each " │ " costs 3 chars. "[RW]" or "[RO]" costs 4.
        let width = area.width as usize;
        let sep_w = 3usize;
        let mode_w = 4usize; // "[RW]" or "[RO]"
        // Required: leading sep + ctx (>=1) + sep + mode_chip + trailing sep
        //   = 3 + 1 + 3 + 4 + 3 = 14 minimum.
        let mut budget = width;
        // Reserve leading + trailing sep + mode chip + sep before mode.
        let reserved = sep_w + sep_w + mode_w + sep_w; // 13
        if budget <= reserved {
            // Degenerate width: just render the mode chip if anything fits.
            let spans = vec![
                Span::styled("[", bracket_style),
                Span::styled(mode_text.to_string(), mode_style),
                Span::styled("]", bracket_style),
            ];
            let line = Line::from(spans);
            let bar = Paragraph::new(line).style(Style::default().bg(bg));
            let rect = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            };
            frame.render_widget(bar, rect);
            return;
        }
        budget -= reserved;

        // Try to keep the context name in full; truncate with '…' if needed.
        let ctx_full = self.context.clone();
        let env_w = env_text.chars().count();
        let layout_w = layout_text.chars().count();

        let mut include_layout = false;
        let mut include_env = false;
        let mut ctx_render = ctx_full.clone();

        // Greedy fit: ctx first (truncated as needed), then env, then layout.
        // After reserved, we have `budget` chars for ctx + optional (sep+env)
        // + optional (sep+layout).
        let ctx_w = ctx_render.chars().count();
        let mut remaining = budget;
        if ctx_w <= remaining {
            remaining -= ctx_w;
        } else {
            // Truncate context to fit, leaving room for '…'.
            ctx_render = truncate_with_ellipsis(&ctx_render, remaining);
            remaining = 0;
        }
        // Try env (costs sep + env_w).
        if remaining >= sep_w + env_w {
            include_env = true;
            remaining -= sep_w + env_w;
        }
        // Try layout (costs sep + layout_w).
        if remaining >= sep_w + layout_w {
            include_layout = true;
        }

        let sep = Span::styled(" │ ", sep_style);
        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(sep.clone());
        spans.push(Span::styled(ctx_render, value_style));
        spans.push(sep.clone());
        if include_env {
            spans.push(Span::styled(env_text, value_style));
            spans.push(sep.clone());
        }
        spans.push(Span::styled("[", bracket_style));
        spans.push(Span::styled(mode_text.to_string(), mode_style));
        spans.push(Span::styled("]", bracket_style));
        spans.push(sep.clone());
        if include_layout {
            spans.push(Span::styled(layout_text, muted_on_bg));
            spans.push(sep);
        }

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().bg(bg));
        let rect = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        frame.render_widget(bar, rect);
    }
```

Add this helper at module scope in `app.rs` (place near the bottom,
before `#[cfg(test)]`):

```rust
fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars == 1 {
        return "…".into();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib app::tests::safety_badge
```
Expected: all 5 `safety_badge_*` tests pass (3 from Task 4 + 2 new).

Then the full suite:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib
```
Expected: `test result: ok. 149 passed`.

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/app.rs
git commit -m "feat(tui): truncation rules for narrow top status bar"
```

---

## Task 6: Add `build_header_chips` helper (no truncation yet)

**Files:**
- Modify: `app/crates/cruster-tui/src/views/dashboard.rs`

Add a free-function helper that takes `Summary`, context name, theme,
and width, and returns two `Line<'static>`s for the chip header. This
task handles the basic chip rendering and `NODES x/y ready` health
zoning; Task 7 adds truncation when width is tight.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `views/dashboard.rs`:

```rust
    fn make_summary(ready: usize, total: usize) -> Summary {
        Summary {
            k8s_version: Some("v1.31.1".into()),
            nodes_total: total,
            nodes_ready: ready,
            namespaces: 11,
            pods_total: 40,
            deployments_total: 16,
            services: 21,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn line_span_with(line: &Line<'_>, needle: &str) -> Option<ratatui::text::Span<'static>> {
        line.spans
            .iter()
            .find(|s| s.content.contains(needle))
            .cloned()
    }

    #[test]
    fn build_header_chips_returns_two_lines_with_all_fields() {
        let theme = crate::theme::Theme::terminal_default();
        let summary = make_summary(3, 3);
        let lines = build_header_chips(&summary, "my-ctx", &theme, 200);
        assert_eq!(lines.len(), 2);
        let l1 = line_text(&lines[0]);
        let l2 = line_text(&lines[1]);
        for needle in ["CONTEXT", "my-ctx", "K8S", "v1.31.1", "NODES", "3/3", "NS", "11"] {
            assert!(l1.contains(needle), "line 1 missing {needle:?}: {l1:?}");
        }
        for needle in ["PODS", "40", "DEPLOYS", "16", "SVCS", "21"] {
            assert!(l2.contains(needle), "line 2 missing {needle:?}: {l2:?}");
        }
    }

    #[test]
    fn build_header_chips_labels_use_chip_label_fg() {
        let theme = crate::theme::Theme::terminal_default();
        let lines = build_header_chips(&make_summary(3, 3), "ctx", &theme, 200);
        let span = line_span_with(&lines[0], "CONTEXT").unwrap();
        assert_eq!(span.style.fg, Some(theme.chip.label_fg.as_ratatui()));
    }

    #[test]
    fn build_header_chips_nodes_value_color_zones() {
        let theme = crate::theme::Theme::terminal_default();

        let lines = build_header_chips(&make_summary(3, 3), "ctx", &theme, 200);
        let span = line_span_with(&lines[0], "3/3").unwrap();
        assert_eq!(span.style.fg, Some(theme.gauge.ok.as_ratatui()));

        let lines = build_header_chips(&make_summary(1, 3), "ctx", &theme, 200);
        let span = line_span_with(&lines[0], "1/3").unwrap();
        assert_eq!(span.style.fg, Some(theme.gauge.warn.as_ratatui()));

        let lines = build_header_chips(&make_summary(0, 3), "ctx", &theme, 200);
        let span = line_span_with(&lines[0], "0/3").unwrap();
        assert_eq!(span.style.fg, Some(theme.gauge.danger.as_ratatui()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard::tests::build_header_chips
```
Expected: 3 tests fail with `cannot find function 'build_header_chips' in this scope`.

- [ ] **Step 3: Implement `build_header_chips`**

Place this helper in `views/dashboard.rs` at module scope (after the
`utilisation_lines` block, before `#[cfg(test)]`):

```rust
/// Builds the two `Line`s of the dashboard chip header (identity row +
/// scale row). Pure — takes a snapshot of `Summary` and renders styled
/// spans against `theme`. `width` is consulted by Task 7 for trailing-
/// chip truncation; in this task all chips render unconditionally.
fn build_header_chips(
    summary: &Summary,
    context_name: &str,
    theme: &Theme,
    _width: u16,
) -> Vec<Line<'static>> {
    let label_style = Style::default()
        .fg(theme.chip.label_fg.as_ratatui())
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default()
        .fg(theme.chip.value_fg.as_ratatui())
        .add_modifier(Modifier::BOLD);
    let gap = Span::raw("   ");

    let nodes_text = format!("{}/{} ready", summary.nodes_ready, summary.nodes_total);
    let nodes_color = if summary.nodes_total == 0 {
        theme.muted_fg.as_ratatui()
    } else if summary.nodes_ready == summary.nodes_total {
        theme.gauge.ok.as_ratatui()
    } else if summary.nodes_ready == 0 {
        theme.gauge.danger.as_ratatui()
    } else {
        theme.gauge.warn.as_ratatui()
    };
    let nodes_value_style = Style::default()
        .fg(nodes_color)
        .add_modifier(Modifier::BOLD);

    let k8s_version = summary
        .k8s_version
        .clone()
        .unwrap_or_else(|| "?".into());

    let identity = Line::from(vec![
        Span::styled("CONTEXT ", label_style),
        Span::styled(context_name.to_string(), value_style),
        gap.clone(),
        Span::styled("K8S ", label_style),
        Span::styled(k8s_version, value_style),
        gap.clone(),
        Span::styled("NODES ", label_style),
        Span::styled(nodes_text, nodes_value_style),
        gap.clone(),
        Span::styled("NS ", label_style),
        Span::styled(summary.namespaces.to_string(), value_style),
    ]);

    let scale = Line::from(vec![
        Span::styled("PODS ", label_style),
        Span::styled(summary.pods_total.to_string(), value_style),
        gap.clone(),
        Span::styled("DEPLOYS ", label_style),
        Span::styled(summary.deployments_total.to_string(), value_style),
        gap,
        Span::styled("SVCS ", label_style),
        Span::styled(summary.services.to_string(), value_style),
    ]);

    vec![identity, scale]
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard::tests::build_header_chips
```
Expected: 3 tests pass.

Full suite:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib
```
Expected: `test result: ok. 152 passed`.

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/views/dashboard.rs
git commit -m "feat(dashboard): build_header_chips helper with health-zoned NODES chip"
```

---

## Task 7: Add truncation to `build_header_chips`

**Files:**
- Modify: `app/crates/cruster-tui/src/views/dashboard.rs`

When the assembled row exceeds `width`, drop trailing chips and append
a muted `…` after the last fitting chip.

- [ ] **Step 1: Write the failing test**

Append to `mod tests`:

```rust
    #[test]
    fn build_header_chips_truncates_trailing_chips_when_narrow() {
        let theme = crate::theme::Theme::terminal_default();
        let summary = make_summary(3, 3);
        // Very narrow: only the CONTEXT chip should survive on line 1.
        let lines = build_header_chips(&summary, "ctx", &theme, 18);
        let l1 = line_text(&lines[0]);
        assert!(l1.contains("CONTEXT"), "CONTEXT must survive: {l1:?}");
        assert!(l1.contains('…'), "ellipsis expected: {l1:?}");
        assert!(!l1.contains("K8S"), "K8S should be dropped: {l1:?}");
    }

    #[test]
    fn build_header_chips_truncation_ellipsis_uses_muted_fg() {
        let theme = crate::theme::Theme::terminal_default();
        let summary = make_summary(3, 3);
        let lines = build_header_chips(&summary, "ctx", &theme, 18);
        let span = line_span_with(&lines[0], "…").unwrap();
        assert_eq!(span.style.fg, Some(theme.muted_fg.as_ratatui()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard::tests::build_header_chips_truncates views::dashboard::tests::build_header_chips_truncation_ellipsis
```
Expected: tests fail — current helper renders all chips regardless of width.

- [ ] **Step 3: Refactor `build_header_chips` to truncate**

Replace the body of `build_header_chips` with a version that builds chips
incrementally and drops them when they would exceed `width`:

```rust
fn build_header_chips(
    summary: &Summary,
    context_name: &str,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let label_style = Style::default()
        .fg(theme.chip.label_fg.as_ratatui())
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default()
        .fg(theme.chip.value_fg.as_ratatui())
        .add_modifier(Modifier::BOLD);
    let ellipsis_style = Style::default().fg(theme.muted_fg.as_ratatui());

    let nodes_text = format!("{}/{} ready", summary.nodes_ready, summary.nodes_total);
    let nodes_color = if summary.nodes_total == 0 {
        theme.muted_fg.as_ratatui()
    } else if summary.nodes_ready == summary.nodes_total {
        theme.gauge.ok.as_ratatui()
    } else if summary.nodes_ready == 0 {
        theme.gauge.danger.as_ratatui()
    } else {
        theme.gauge.warn.as_ratatui()
    };
    let nodes_value_style = Style::default()
        .fg(nodes_color)
        .add_modifier(Modifier::BOLD);

    let k8s_version = summary
        .k8s_version
        .clone()
        .unwrap_or_else(|| "?".into());

    // Each chip = (label_with_space, value, value_style). The label
    // always carries a trailing space; chip widths are computed inline.
    let identity_chips: Vec<(&str, String, Style)> = vec![
        ("CONTEXT ", context_name.to_string(), value_style),
        ("K8S ", k8s_version, value_style),
        ("NODES ", nodes_text, nodes_value_style),
        ("NS ", summary.namespaces.to_string(), value_style),
    ];
    let scale_chips: Vec<(&str, String, Style)> = vec![
        ("PODS ", summary.pods_total.to_string(), value_style),
        ("DEPLOYS ", summary.deployments_total.to_string(), value_style),
        ("SVCS ", summary.services.to_string(), value_style),
    ];

    let identity = fit_chips_into_line(&identity_chips, label_style, ellipsis_style, width);
    let scale = fit_chips_into_line(&scale_chips, label_style, ellipsis_style, width);
    vec![identity, scale]
}

/// Fit chips left-to-right within `width`. If a chip can't fit, stop and
/// append a muted `…` (only when at least one chip was placed and at
/// least one chip was dropped).
fn fit_chips_into_line(
    chips: &[(&str, String, Style)],
    label_style: Style,
    ellipsis_style: Style,
    width: u16,
) -> Line<'static> {
    let gap = "   ";
    let gap_w = gap.chars().count();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used: usize = 0;
    let mut placed = 0usize;

    for (i, (label, value, value_style)) in chips.iter().enumerate() {
        let chip_w = label.chars().count() + value.chars().count();
        let needs_gap = i > 0;
        let total_w = if needs_gap { gap_w + chip_w } else { chip_w };
        if used + total_w > width as usize {
            break;
        }
        if needs_gap {
            spans.push(Span::raw(gap.to_string()));
        }
        spans.push(Span::styled(label.to_string(), label_style));
        spans.push(Span::styled(value.clone(), *value_style));
        used += total_w;
        placed += 1;
    }

    let dropped = chips.len().saturating_sub(placed);
    if dropped > 0 && placed > 0 {
        // Append ellipsis only if it actually fits; otherwise leave it off.
        let suffix = format!("{gap}…");
        if used + suffix.chars().count() <= width as usize {
            spans.push(Span::raw(gap.to_string()));
            spans.push(Span::styled("…".to_string(), ellipsis_style));
        }
    }

    Line::from(spans)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard::tests::build_header_chips
```
Expected: all 5 `build_header_chips_*` tests pass (3 from Task 6 + 2 new).

Full suite:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib
```
Expected: `test result: ok. 154 passed`.

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/views/dashboard.rs
git commit -m "feat(dashboard): truncate trailing chips with muted ellipsis"
```

---

## Task 8: Wire `build_header_chips` into `render_summary`

**Files:**
- Modify: `app/crates/cruster-tui/src/views/dashboard.rs`

Replace the construction of `line1` (cluster identity) and `line2`
(scale counts) inside `render_summary` with two calls to
`build_header_chips`. CPU/MEM lines remain unchanged.

- [ ] **Step 1: Write the failing integration test**

Append to `mod tests`:

```rust
    #[test]
    fn render_summary_uses_chip_header() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut v = DashboardView::new();
        v.config = DashboardConfig::default(); // hermetic
        v.summary = make_summary(3, 3);
        v.context_name = "my-ctx".into();
        let theme = crate::theme::Theme::terminal_default();
        let backend = TestBackend::new(120, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| v.render_summary(f, f.area(), &theme))
            .unwrap();
        let text = buffer_to_string(terminal.backend().buffer());
        for needle in [
            "CONTEXT", "my-ctx", "K8S", "v1.31.1",
            "NODES", "3/3", "NS", "11",
            "PODS", "40", "DEPLOYS", "16", "SVCS", "21",
        ] {
            assert!(text.contains(needle), "summary missing {needle:?} in:\n{text}");
        }
    }
```

`render_summary` is a method on `DashboardView`. The test mutates the
view's private fields directly, which works because the test is in the
same module. `DashboardConfig` is imported at the top of the file via
the existing `use crate::dashboard::{DashboardConfig, Pin};`.

- [ ] **Step 2: Run test to verify it fails**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard::tests::render_summary_uses_chip_header
```
Expected: assertion failure — `CONTEXT` not found (current summary uses
the muted "cluster X · vY · n nodes ready · m ns" line format with no
"CONTEXT" label).

- [ ] **Step 3: Replace `line1` / `line2` construction with helper call**

In `views/dashboard.rs::render_summary`, locate the current
construction:

```rust
        // Line 1: cluster identity — name + version + node + namespace counts.
        let line1 = Line::from(vec![ /* … */ ]);
        // Line 2: scale — workload object counts. Movable but slow.
        let line2 = Line::from(vec![ /* … */ ]);
        // Lines 3-4: cluster CPU / MEM utilisation from metrics-server.
        let (line3, line4) = utilisation_lines(&self.metrics, area.width, theme);

        let para = Paragraph::new(vec![line1, line2, line3, line4]).block( /* … */ );
```

Replace `line1` / `line2` with a single call to `build_header_chips`:

```rust
        let header = build_header_chips(&self.summary, &self.context_name, theme, area.width);
        let (line3, line4) = utilisation_lines(&self.metrics, area.width, theme);

        let mut lines = header;
        lines.push(line3);
        lines.push(line4);

        let para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" pulse · {} pins ", self.config.pins.len())),
        );
        frame.render_widget(para, area);
```

Remove the now-unused `line1` / `line2` blocks and any local variables
that fed only into them (`s`, `version` are still used for the chip
helper indirectly — wait, no: `summary` is `self.summary`, passed by
reference to the helper; `version` and `s` lookups are no longer
needed). Drop the dead bindings.

- [ ] **Step 4: Run tests to verify they pass**

Run:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard::tests::render_summary_uses_chip_header
```
Expected: pass.

Then run the existing render-tests to verify nothing else regressed
(`render_metrics_available_shows_cpu_and_mem_lines`, etc. — those test
the CPU/MEM lines that are still rendered):

```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib views::dashboard
```
Expected: all dashboard view tests pass.

Full suite:
```
cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib
```
Expected: `test result: ok. 155 passed`.

- [ ] **Step 5: Commit**

```
git add app/crates/cruster-tui/src/views/dashboard.rs
git commit -m "feat(dashboard): wire chip header into summary band"
```

---

## Task 9: Manual smoke test in pane 2

**Files:** none (verification only)

- [ ] **Step 1: Rebuild release-ish binary**

Run:
```
cargo build --manifest-path app/Cargo.toml
```
Expected: finished cleanly.

- [ ] **Step 2: Capture pane 2 (the running TUI, pre-restart)**

Run:
```
tmux capture-pane -t :1.2 -p | head -10
```
This will still show the old binary's output — that's expected. The
purpose is to confirm pane 2 is the cruster TUI session and not
something else.

- [ ] **Step 3: Ask the user to restart pane 2**

Tell the user: "Pane 2 is still running the previous binary. Focus pane
2, press `q` to quit, then re-run whichever command you used to launch
cruster (likely `cargo run --manifest-path app/Cargo.toml -p cruster-bin`
or `./app/target/debug/cruster`). Let me know when it's back up and I'll
re-capture."

- [ ] **Step 4: Re-capture and verify visually**

After user confirms restart:
```
tmux capture-pane -t :1.2 -p | head -10
```
Expected output (text-only — colors not visible here):
- Top row shows `│ <ctx> │ UNKNOWN │ [RW] │ single │` (uppercase env, square-bracket mode chip, `│` separators).
- Summary band shows `CONTEXT <ctx>   K8S vX.Y.Z   NODES n/m ready   NS k` on one row and `PODS …   DEPLOYS …   SVCS …` on the next.

If the visual matches, the implementation works. Report success.

If something looks wrong (missing chips, wrong spacing, ellipsis where
none expected), inspect the captured row and decide whether to fix
inline or pause for feedback.

---

## Done criteria

- All 155+ cruster-tui lib tests pass (`cargo test --manifest-path app/Cargo.toml -p cruster-tui --lib`).
- Pane 2 visually confirms the new chip header and brightened top bar.
- Spec keys (`env_band_fg`, `mode`, `chip`) are all present on
  `Theme::terminal_default()` and on every `Theme::embedded(name)`.
- No hex literals or hardcoded `Color::Black`/`Color::White`/etc. in
  the touched render paths — every color cites a theme key.
