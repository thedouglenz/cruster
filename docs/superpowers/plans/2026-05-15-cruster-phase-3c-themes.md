# Cruster Phase 3C: Theme Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cruster gets a first-class, shareable theme system. A
`Theme` struct holds every color/border/glyph/style decision used by
the renderer. Themes are TOML files. The default `terminal` theme
ships with the binary and inherits the host palette (free tier). A
bundled library (`dark`, `light`, `solarized-dark`, `solarized-light`,
`monokai`, `gruvbox`, `tokyonight`, `catppuccin`) ships embedded and
unlocks with Pro. Users can install custom themes by dropping a TOML
into `~/.config/cruster/themes/`. Live reload via
`cruster --watch-theme <path>` for theme authors. `cruster theme
preview <path>` (CLI) renders the theme to non-TTY for review.

**Pricing gate:** theme switching, the bundled library, and
`cruster theme install` are Pro features. The `terminal` default
works in Free tier. Free users see the palette/discoverability with
the default; selecting any other theme shows "Pro required" toast.

For Phase 3C there's no real license check yet (Phase 5 ships that).
We add a `Tier` enum with a `Free` default; the gate hook is in
place but always unlocks for now. Phase 5 wires it to real licensing.

This is sub-phase 3 of Phase 3. Sub-phase 3D follows: keymap presets
and layouts.

**Architecture:**
- New `cruster-tui` module `theme.rs`: `Theme` struct, all colors +
  glyphs the renderer needs; `from_toml(path)` + `from_embedded(name)`.
- All TUI render code now reads from `&Theme` instead of using
  hardcoded `Color::DarkGray` etc. App holds `theme: Theme`.
- New `cruster-cli` verb `cruster theme preview <path>` renders a
  representative TUI screen to text for non-TTY inspection.
- 8 bundled themes embedded via `include_str!`.
- Pro gate stub: `tier.rs` in `cruster-core` with a Tier enum.

---

## File Structure

New files:
```
app/crates/cruster-core/src/tier.rs                     # Tier enum
app/crates/cruster-tui/src/theme.rs                     # Theme struct + loader
app/crates/cruster-tui/themes/terminal.toml             # default (free)
app/crates/cruster-tui/themes/dark.toml                 # Pro
app/crates/cruster-tui/themes/light.toml                # Pro
app/crates/cruster-tui/themes/solarized-dark.toml       # Pro
app/crates/cruster-tui/themes/solarized-light.toml      # Pro
app/crates/cruster-tui/themes/monokai.toml              # Pro
app/crates/cruster-tui/themes/gruvbox.toml              # Pro
app/crates/cruster-tui/themes/tokyonight.toml           # Pro
app/crates/cruster-tui/themes/catppuccin.toml           # Pro
app/crates/cruster-cli/src/verbs/theme.rs               # cruster theme preview/list
```

Modifications:
- `cruster-core/src/lib.rs` — re-export `tier`
- `cruster-tui/src/lib.rs` — re-export `theme`
- `cruster-tui/src/app.rs` — hold `theme`, pass to render helpers, gate switching
- All view render fns + overlay render fns — read colors from `Theme`
  (passed via `App`'s render helpers; for now keep it simple: views
  receive `&Theme` indirectly through the App's render layout)
- `cruster-cli/src/args.rs` — `Theme(ThemeArgs)` command
- `cruster-cli/src/verbs/mod.rs` — dispatch

---

## Task 1: Tier enum

Tiny shared type for the Pro gate. Lives in core because both `tui`
(for in-app gating) and `cli` (for `cruster theme install`'s
gating in Phase 5) will need it.

- [ ] Add `app/crates/cruster-core/src/tier.rs`:

```rust
//! Subscription tier: gates Pro features. Phase 5 wires this to a
//! real license file; for Phase 3C it's hardcoded `Free` everywhere.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    #[default]
    Free,
    Pro,
    Team,
    Enterprise,
}

impl Tier {
    /// Whether this tier unlocks features marked "Pro+".
    pub fn has_pro(self) -> bool {
        !matches!(self, Self::Free)
    }
}
```

- [ ] Add `pub mod tier;` to `cruster-core/src/lib.rs` + a test.
- [ ] Commit: `feat(core): add Tier enum for Pro feature gating`.

---

## Task 2: Theme struct + TOML loader

The `Theme` struct holds every renderer-controlled decision: colors
for selected row, command-line bar, action footer, env badges per
environment, status indicators per pod phase, borders, list highlight,
search bar, etc.

- [ ] Create `app/crates/cruster-tui/src/theme.rs`:

```rust
//! TUI theme: every color/style decision the renderer makes.

use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Theme {
    /// Human label shown in the palette.
    pub name: String,
    /// Background color of the action footer.
    #[serde(default = "default_footer_bg")]
    pub footer_bg: ThemeColor,
    /// Background of the command-mode prompt.
    #[serde(default = "default_command_bg")]
    pub command_bg: ThemeColor,
    /// Background of the search prompt.
    #[serde(default = "default_search_bg")]
    pub search_bg: ThemeColor,
    /// Toast (info) background.
    #[serde(default = "default_toast_bg")]
    pub toast_bg: ThemeColor,
    /// Selected-row highlight.
    #[serde(default = "default_selected_bg")]
    pub selected_bg: ThemeColor,
    /// Per-environment safety badge colors.
    #[serde(default)]
    pub env_band: EnvBand,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvBand {
    #[serde(default = "default_prod")]
    pub prod: ThemeColor,
    #[serde(default = "default_staging")]
    pub staging: ThemeColor,
    #[serde(default = "default_dev")]
    pub dev: ThemeColor,
    #[serde(default = "default_local")]
    pub local: ThemeColor,
    #[serde(default = "default_unknown")]
    pub unknown: ThemeColor,
}

impl Default for EnvBand {
    fn default() -> Self {
        Self {
            prod: default_prod(),
            staging: default_staging(),
            dev: default_dev(),
            local: default_local(),
            unknown: default_unknown(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ThemeColor {
    Named(String),
    Reset,
}

impl ThemeColor {
    pub fn as_ratatui(&self) -> Color {
        match self {
            Self::Reset => Color::Reset,
            Self::Named(s) => match s.as_str() {
                "reset" => Color::Reset,
                "black" => Color::Black,
                "red" => Color::Red,
                "green" => Color::Green,
                "yellow" => Color::Yellow,
                "blue" => Color::Blue,
                "magenta" => Color::Magenta,
                "cyan" => Color::Cyan,
                "white" => Color::Gray,
                "gray" | "grey" => Color::Gray,
                "darkgray" | "darkgrey" => Color::DarkGray,
                "lightred" => Color::LightRed,
                "lightgreen" => Color::LightGreen,
                "lightyellow" => Color::LightYellow,
                "lightblue" => Color::LightBlue,
                "lightmagenta" => Color::LightMagenta,
                "lightcyan" => Color::LightCyan,
                hex if hex.starts_with('#') && hex.len() == 7 => parse_hex(hex)
                    .unwrap_or(Color::Reset),
                _ => Color::Reset,
            },
        }
    }
}

fn parse_hex(s: &str) -> Option<Color> {
    let r = u8::from_str_radix(&s[1..3], 16).ok()?;
    let g = u8::from_str_radix(&s[3..5], 16).ok()?;
    let b = u8::from_str_radix(&s[5..7], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn default_footer_bg() -> ThemeColor { ThemeColor::Named("darkgray".into()) }
fn default_command_bg() -> ThemeColor { ThemeColor::Named("darkgray".into()) }
fn default_search_bg() -> ThemeColor { ThemeColor::Named("blue".into()) }
fn default_toast_bg() -> ThemeColor { ThemeColor::Named("red".into()) }
fn default_selected_bg() -> ThemeColor { ThemeColor::Named("darkgray".into()) }
fn default_prod() -> ThemeColor { ThemeColor::Named("red".into()) }
fn default_staging() -> ThemeColor { ThemeColor::Named("yellow".into()) }
fn default_dev() -> ThemeColor { ThemeColor::Named("green".into()) }
fn default_local() -> ThemeColor { ThemeColor::Named("cyan".into()) }
fn default_unknown() -> ThemeColor { ThemeColor::Named("darkgray".into()) }

impl Theme {
    pub fn terminal_default() -> Self {
        toml::from_str(include_str!("../themes/terminal.toml"))
            .expect("embedded terminal theme must parse")
    }

    pub fn embedded(name: &str) -> Option<Self> {
        let body = match name {
            "terminal" => include_str!("../themes/terminal.toml"),
            "dark" => include_str!("../themes/dark.toml"),
            "light" => include_str!("../themes/light.toml"),
            "solarized-dark" => include_str!("../themes/solarized-dark.toml"),
            "solarized-light" => include_str!("../themes/solarized-light.toml"),
            "monokai" => include_str!("../themes/monokai.toml"),
            "gruvbox" => include_str!("../themes/gruvbox.toml"),
            "tokyonight" => include_str!("../themes/tokyonight.toml"),
            "catppuccin" => include_str!("../themes/catppuccin.toml"),
            _ => return None,
        };
        toml::from_str(body).ok()
    }

    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        let body = std::fs::read_to_string(path)?;
        let theme: Self = toml::from_str(&body)?;
        Ok(theme)
    }

    /// Names of all bundled themes, in display order.
    pub fn bundled_names() -> &'static [&'static str] {
        &[
            "terminal",
            "dark",
            "light",
            "solarized-dark",
            "solarized-light",
            "monokai",
            "gruvbox",
            "tokyonight",
            "catppuccin",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_default_parses() {
        let _ = Theme::terminal_default();
    }

    #[test]
    fn all_bundled_themes_parse() {
        for name in Theme::bundled_names() {
            let t = Theme::embedded(name)
                .unwrap_or_else(|| panic!("embedded theme '{name}' missing"));
            assert_eq!(t.name.to_lowercase().contains(name) || !t.name.is_empty(), true);
        }
    }

    #[test]
    fn hex_colors_parse() {
        let c = ThemeColor::Named("#ff0080".into()).as_ratatui();
        assert_eq!(c, Color::Rgb(0xff, 0x00, 0x80));
    }

    #[test]
    fn named_colors_map_correctly() {
        assert_eq!(ThemeColor::Named("red".into()).as_ratatui(), Color::Red);
        assert_eq!(
            ThemeColor::Named("darkgray".into()).as_ratatui(),
            Color::DarkGray
        );
    }
}
```

- [ ] Create the 9 TOML files (`terminal.toml`, `dark.toml`, etc.).
  Minimal example — `terminal.toml`:

  ```toml
  name = "Terminal (default)"
  ```

  Empty defaults pull through the `serde(default)` helpers, which use
  the host terminal's `Color::Reset` … actually `default_*` return
  `darkgray` etc. For `terminal` we want `Color::Reset` everywhere.
  Override:

  ```toml
  name = "Terminal (default)"
  footer_bg = "reset"
  command_bg = "reset"
  search_bg = "reset"
  toast_bg = "reset"
  selected_bg = "reset"

  [env_band]
  prod = "reset"
  staging = "reset"
  dev = "reset"
  local = "reset"
  unknown = "reset"
  ```

  The other themes specify real colors. For brevity, `dark.toml`:
  ```toml
  name = "Dark"
  footer_bg = "darkgray"
  command_bg = "darkgray"
  search_bg = "blue"
  toast_bg = "red"
  selected_bg = "#444444"

  [env_band]
  prod = "#cc0000"
  staging = "#ccaa00"
  dev = "#00aa00"
  local = "#00aaaa"
  unknown = "#666666"
  ```

  (Each remaining theme has its own palette in the same shape.)

- [ ] Register `pub mod theme;` in `cruster-tui/src/lib.rs` and
  re-export `Theme`. Test and commit.

---

## Task 3: Wire Theme into App + rendering

- [ ] App holds `theme: Theme`. Initialised to `Theme::terminal_default()`.
- [ ] `tier: Tier` field on App, initialised to `Tier::default()` (Free).
- [ ] All places that currently use hardcoded `Color::*` in render
  functions take colors from `self.theme.<field>.as_ratatui()`
  instead. The colors touched (per Phase 3A wiring): action footer
  bg, command-line bg, search bg, toast bg, env badge bg, selected
  row bg.
- [ ] Add an action `theme` (key chord `T`?) that opens a Palette
  filtered to `Theme::bundled_names()`. Switching is gated:
  - If `self.tier.has_pro()` is true: switch + toast "theme: <name>"
  - Else: toast "themes are a Pro feature"
- [ ] Test + commit.

---

## Task 4: `cruster theme` CLI verb

`cruster theme list` — list bundled themes.
`cruster theme preview <name>` — print a representative TUI render
of the theme to stdout (text only, no colors — for sanity-checking
TOML in CI). For Phase 3C this is "show the theme's resolved color
values"; full visual preview lands later.

- [ ] Add `Theme(ThemeArgs)` to `args.rs` with subcommands `list`
  and `preview <name>`.
- [ ] Implement in `cruster-cli/src/verbs/theme.rs`:
  - `list`: print each bundled theme name to stdout, one per line
    (or JSON/YAML per format).
  - `preview <name>`: load the theme, print its resolved color names
    keyed by field. Useful for debugging.
- [ ] Schema file + register. Commit.

---

## Task 5: `cruster theme install <url>` placeholder

The full implementation (download from URL, validate, write to
`~/.config/cruster/themes/`) is Phase 5 work. For 3C, the verb
exists but returns a "Pro feature — not yet implemented in v1"
message. The point is to establish the verb shape early.

- [ ] Add to args + verb dispatch. Bail with the message. Commit.

---

## Task 6: Phase 3C exit verification

- [ ] fmt + clippy + test.
- [ ] Manually verify against k3d:
  - TUI launches with the default `terminal` theme.
  - `cruster theme list` prints 9 names.
  - `cruster theme preview dark` prints theme color fields.
  - In TUI, `T` opens theme palette; switching to anything other than
    `terminal` shows "Pro feature" toast (since Tier::Free is
    hardcoded).
- [ ] Update READMEs.
- [ ] Tag `phase-3c-themes`.

---

## Phase 3C exit criteria

1. `cargo test --workspace` passes.
2. clippy/fmt clean.
3. 9 bundled themes parse.
4. Pro gate fires correctly for Free users.
5. CI green.

When all hold, write Phase 3D (keymap presets + layouts).
