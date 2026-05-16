# Cruster Phase 3D: Keymap Presets + Named Layouts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Users pick from three first-class keymap presets — `vim`,
`emacs`, `normal` — without per-binding configuration. Named
multi-pane layouts (`single`, `triplet`, `incident`) switch with
`Alt+1..3` and arrange the body of the screen for different
incident-investigation styles. This completes Phase 3.

**Architecture:**
- New `cruster-tui/src/keymap.rs`: a `Keymap` struct mapping
  semantic actions (`MoveDown`, `Quit`, `OpenPalette`, etc.) to
  concrete `KeyCode` chords. Three preset constructors:
  `Keymap::vim()`, `Keymap::emacs()`, `Keymap::normal()`. Default:
  `normal`. App resolves incoming `KeyEvent` through the keymap
  before dispatching.
- New `cruster-tui/src/layout.rs`: `Layout` enum with
  `Single` (current behaviour), `Triplet` (top list, bottom-left
  describe pane, bottom-right logs pane), `Incident` (top events
  stream, middle list, bottom logs of selected). `App` holds the
  active layout; `render_full` switches on it.
- Config file: `~/.config/cruster/keymap.toml` selects preset and
  optionally overrides individual bindings.

For Phase 3D v1, the Triplet and Incident layouts reuse the
existing describe/logs panes but auto-open them based on selection.
A full multi-pane refactor (where every cell renders its own
selectable view) is post-v1.

---

## Task 1: Keymap module + 3 presets

- [ ] Create `app/crates/cruster-tui/src/keymap.rs`:

```rust
//! Keymap presets: vim / emacs / normal.
//!
//! A Keymap resolves a (KeyCode, KeyModifiers) pair to a semantic
//! Action. App's key dispatcher routes via the keymap so users can
//! swap presets without recompiling.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyModifiers};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticAction {
    Quit,
    MoveUp,
    MoveDown,
    MoveTop,
    MoveBottom,
    OpenPalette,
    OpenSearch,
    OpenCommandMode,
    OpenHistory,
    OpenWorkflows,
    OpenThemes,
    OpenRelationships,
    Describe,
    Logs,
    Exec,
    PortForward,
    EditYaml,
    CopyKubectl,
    ToggleReadOnly,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Normal,
    Vim,
    Emacs,
}

impl Default for Preset {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<(KeyCode, KeyModifiers), SemanticAction>,
}

impl Keymap {
    pub fn for_preset(preset: Preset) -> Self {
        match preset {
            Preset::Normal => Self::normal(),
            Preset::Vim => Self::vim(),
            Preset::Emacs => Self::emacs(),
        }
    }

    pub fn normal() -> Self {
        let mut m = HashMap::new();
        // Same as our existing default — vim-style movement + simple actions.
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        m.insert((KeyCode::Char('q'), none), SemanticAction::Quit);
        m.insert((KeyCode::Esc, none), SemanticAction::Quit);
        m.insert((KeyCode::Char('j'), none), SemanticAction::MoveDown);
        m.insert((KeyCode::Down, none), SemanticAction::MoveDown);
        m.insert((KeyCode::Char('k'), none), SemanticAction::MoveUp);
        m.insert((KeyCode::Up, none), SemanticAction::MoveUp);
        m.insert((KeyCode::Char('g'), none), SemanticAction::MoveTop);
        m.insert((KeyCode::Home, none), SemanticAction::MoveTop);
        m.insert((KeyCode::Char('G'), none), SemanticAction::MoveBottom);
        m.insert((KeyCode::End, none), SemanticAction::MoveBottom);
        m.insert((KeyCode::Char('p'), ctrl), SemanticAction::OpenPalette);
        m.insert((KeyCode::Char('/'), none), SemanticAction::OpenSearch);
        m.insert((KeyCode::Char(':'), none), SemanticAction::OpenCommandMode);
        m.insert((KeyCode::Char('H'), none), SemanticAction::OpenHistory);
        m.insert((KeyCode::Char('W'), none), SemanticAction::OpenWorkflows);
        m.insert((KeyCode::Char('T'), none), SemanticAction::OpenThemes);
        m.insert((KeyCode::Char('r'), none), SemanticAction::OpenRelationships);
        m.insert((KeyCode::Char('d'), none), SemanticAction::Describe);
        m.insert((KeyCode::Char('y'), none), SemanticAction::Describe);
        m.insert((KeyCode::Char('l'), none), SemanticAction::Logs);
        m.insert((KeyCode::Char('s'), none), SemanticAction::Exec);
        m.insert((KeyCode::Char('f'), none), SemanticAction::PortForward);
        m.insert((KeyCode::Char('e'), none), SemanticAction::EditYaml);
        m.insert((KeyCode::Char('K'), none), SemanticAction::CopyKubectl);
        m.insert((KeyCode::Char('r'), ctrl), SemanticAction::ToggleReadOnly);
        Self { bindings: m }
    }

    pub fn vim() -> Self {
        // Vim is largely the same as Normal for cruster (we already
        // use hjkl). Differences: g+g for top instead of g; n/N for
        // next/prev search (out of scope for v1).
        Self::normal()
    }

    pub fn emacs() -> Self {
        let mut m = Self::normal().bindings;
        let ctrl = KeyModifiers::CONTROL;
        let none = KeyModifiers::NONE;
        // Override hjkl-style with Emacs movement.
        m.insert((KeyCode::Char('n'), ctrl), SemanticAction::MoveDown);
        m.insert((KeyCode::Char('p'), ctrl), SemanticAction::MoveUp);
        m.insert((KeyCode::Char('a'), ctrl), SemanticAction::MoveTop);
        m.insert((KeyCode::Char('e'), ctrl), SemanticAction::MoveBottom);
        // Emacs needs a non-Ctrl-P binding for palette; use Alt-x.
        m.insert((KeyCode::Char('x'), KeyModifiers::ALT), SemanticAction::OpenPalette);
        // Restore j/k for muscle-memory.
        m.insert((KeyCode::Char('j'), none), SemanticAction::MoveDown);
        m.insert((KeyCode::Char('k'), none), SemanticAction::MoveUp);
        Self { bindings: m }
    }

    pub fn resolve(&self, code: KeyCode, modifiers: KeyModifiers) -> Option<SemanticAction> {
        self.bindings.get(&(code, modifiers)).copied()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KeymapConfig {
    #[serde(default)]
    pub preset: Preset,
}

impl KeymapConfig {
    pub fn load_or_default() -> Self {
        let Some(mut p) = dirs::config_dir() else {
            return Self::default();
        };
        p.push("cruster");
        p.push("keymap.toml");
        let Ok(body) = std::fs::read_to_string(p) else {
            return Self::default();
        };
        toml::from_str(&body).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_resolves_j_to_movedown() {
        let k = Keymap::normal();
        assert_eq!(
            k.resolve(KeyCode::Char('j'), KeyModifiers::NONE),
            Some(SemanticAction::MoveDown)
        );
    }

    #[test]
    fn emacs_resolves_ctrl_n_to_movedown() {
        let k = Keymap::emacs();
        assert_eq!(
            k.resolve(KeyCode::Char('n'), KeyModifiers::CONTROL),
            Some(SemanticAction::MoveDown)
        );
    }

    #[test]
    fn unknown_key_resolves_to_none() {
        let k = Keymap::normal();
        assert_eq!(k.resolve(KeyCode::Char('z'), KeyModifiers::NONE), None);
    }
}
```

- [ ] Re-export from `lib.rs`. Test + commit:
  `feat(tui): add keymap presets (normal/vim/emacs) + KeymapConfig loader`.

---

## Task 2: Layout enum

- [ ] Create `app/crates/cruster-tui/src/layout.rs`:

```rust
//! Named multi-pane layouts.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// Single pane: just the current view.
    Single,
    /// Top half: view. Bottom half: describe pane (always open).
    Triplet,
    /// Top: events. Middle: list. Bottom: logs of selected.
    Incident,
}

impl Default for Layout {
    fn default() -> Self {
        Self::Single
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_single() {
        assert_eq!(Layout::default(), Layout::Single);
    }
}
```

For Phase 3D v1, only the Layout enum + `Alt+1/2/3` switching ships.
The actual `Triplet`/`Incident` rendering is wired up but mostly
reuses the existing describe+logs panes (auto-opened in the layout
when the user selects a pod). A full multi-pane refactor (each cell
its own selectable view) is post-v1.

- [ ] Re-export + test + commit.

---

## Task 3: App integration

- [ ] App holds `keymap: Keymap` + `layout: Layout`.
- [ ] `handle_key` routes through `self.keymap.resolve(...)`; if a
  semantic action returns, dispatch it via `invoke_action`. If None,
  fall through to current behaviour.
- [ ] Add the missing semantic actions to `invoke_action` (the ones
  not already wired through existing handlers).
- [ ] `Alt+1` → Layout::Single; `Alt+2` → Triplet; `Alt+3` → Incident.
- [ ] `render_full` switches on layout. Triplet renders the view in
  the top half and the describe pane in the bottom half (auto-open
  describe if a row is selected). Incident does the same with logs
  on the bottom.
- [ ] Commit: `feat(tui): wire keymap presets + named layouts (Alt+1..3)`.

---

## Task 4: Phase 3D exit verification + Phase 3 closeout

- [ ] fmt + clippy + test
- [ ] Manual k3d test:
  - Set `~/.config/cruster/keymap.toml = preset = "emacs"`, restart,
    confirm Ctrl-N moves down
  - `Alt+2` switches to triplet layout
- [ ] Update READMEs (top-level + app)
- [ ] Tag `phase-3d-keymaps-layouts` AND `phase-3-complete`
- [ ] Commit

---

## Phase 3 closeout

After 3D ships, all of Phase 3 (incident-solving ergonomics +
themes) is done. Next phase is **Phase 4** (agent surface — prompt
actions, diagnostic export, agentskills.io skills, Claude Code
plugin).
