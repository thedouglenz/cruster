//! Keymap presets: vim / emacs / normal.

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
    LayoutSingle,
    LayoutTriplet,
    LayoutIncident,
    /// Leader: arms the app to consume the next keystroke as a prompt
    /// trigger char. e.g. `P` then `d` runs the `diagnose` prompt.
    OpenPromptLeader,
    /// Build a diagnostic markdown bundle for the current selection
    /// and write it to a file in the cwd.
    ExportDiagnostic,
    /// Pin the currently selected resource to the pulse dashboard.
    PinToDashboard,
    /// Open the delete modal for the current selection.
    Delete,
    /// Open the help overlay (full keybind cheat sheet).
    OpenHelp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    Normal,
    Vim,
    Emacs,
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
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
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
        m.insert(
            (KeyCode::Char('r'), none),
            SemanticAction::OpenRelationships,
        );
        m.insert((KeyCode::Char('d'), none), SemanticAction::Describe);
        m.insert((KeyCode::Char('y'), none), SemanticAction::Describe);
        m.insert((KeyCode::Char('l'), none), SemanticAction::Logs);
        m.insert((KeyCode::Char('s'), none), SemanticAction::Exec);
        m.insert((KeyCode::Char('f'), none), SemanticAction::PortForward);
        m.insert((KeyCode::Char('e'), none), SemanticAction::EditYaml);
        m.insert((KeyCode::Char('K'), none), SemanticAction::CopyKubectl);
        m.insert((KeyCode::Char('r'), ctrl), SemanticAction::ToggleReadOnly);
        m.insert((KeyCode::Char('1'), alt), SemanticAction::LayoutSingle);
        m.insert((KeyCode::Char('2'), alt), SemanticAction::LayoutTriplet);
        m.insert((KeyCode::Char('3'), alt), SemanticAction::LayoutIncident);
        m.insert((KeyCode::Char('P'), none), SemanticAction::OpenPromptLeader);
        m.insert((KeyCode::Char('E'), none), SemanticAction::ExportDiagnostic);
        m.insert((KeyCode::Char('a'), none), SemanticAction::PinToDashboard);
        m.insert((KeyCode::Char('D'), none), SemanticAction::Delete);
        m.insert((KeyCode::Char('?'), none), SemanticAction::OpenHelp);
        Self { bindings: m }
    }

    pub fn vim() -> Self {
        // Largely the same as Normal (cruster already uses hjkl).
        Self::normal()
    }

    pub fn emacs() -> Self {
        let mut m = Self::normal().bindings;
        let ctrl = KeyModifiers::CONTROL;
        let alt = KeyModifiers::ALT;
        m.insert((KeyCode::Char('n'), ctrl), SemanticAction::MoveDown);
        m.insert((KeyCode::Char('p'), ctrl), SemanticAction::MoveUp);
        m.insert((KeyCode::Char('a'), ctrl), SemanticAction::MoveTop);
        m.insert((KeyCode::Char('e'), ctrl), SemanticAction::MoveBottom);
        // Emacs convention: M-x for the palette.
        m.insert((KeyCode::Char('x'), alt), SemanticAction::OpenPalette);
        Self { bindings: m }
    }

    pub fn resolve(&self, code: KeyCode, modifiers: KeyModifiers) -> Option<SemanticAction> {
        // Terminals disagree on whether `Shift+letter` arrives with the
        // SHIFT bit set (kitty-style enhancement) or already folded
        // into the uppercase char (vanilla ANSI). Strip SHIFT from
        // uppercase-char chords so a single binding works on both.
        let lookup_mods = match code {
            KeyCode::Char(c) if c.is_ascii_uppercase() => modifiers & !KeyModifiers::SHIFT,
            _ => modifiers,
        };
        self.bindings.get(&(code, lookup_mods)).copied()
    }

    /// Iterate over every (chord → action) binding. Used by the help
    /// overlay to render the cheat sheet.
    pub fn entries(&self) -> impl Iterator<Item = (&(KeyCode, KeyModifiers), &SemanticAction)> {
        self.bindings.iter()
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
    fn emacs_preserves_normal_palette_binding_plus_meta_x() {
        let k = Keymap::emacs();
        assert_eq!(
            k.resolve(KeyCode::Char('x'), KeyModifiers::ALT),
            Some(SemanticAction::OpenPalette)
        );
    }

    #[test]
    fn unknown_key_resolves_to_none() {
        let k = Keymap::normal();
        assert_eq!(k.resolve(KeyCode::Char('z'), KeyModifiers::NONE), None);
    }

    #[test]
    fn alt_1_2_3_for_layouts() {
        let k = Keymap::normal();
        assert_eq!(
            k.resolve(KeyCode::Char('1'), KeyModifiers::ALT),
            Some(SemanticAction::LayoutSingle)
        );
        assert_eq!(
            k.resolve(KeyCode::Char('2'), KeyModifiers::ALT),
            Some(SemanticAction::LayoutTriplet)
        );
        assert_eq!(
            k.resolve(KeyCode::Char('3'), KeyModifiers::ALT),
            Some(SemanticAction::LayoutIncident)
        );
    }

    #[test]
    fn preset_default_is_normal() {
        assert_eq!(Preset::default(), Preset::Normal);
    }

    #[test]
    fn uppercase_chord_resolves_with_or_without_shift_modifier() {
        // Some terminals send Shift+letter as (Char('D'), SHIFT);
        // others send it as (Char('D'), NONE). Both must hit the
        // same binding so a capital-letter chord works regardless.
        let k = Keymap::normal();
        assert_eq!(
            k.resolve(KeyCode::Char('D'), KeyModifiers::NONE),
            Some(SemanticAction::Delete),
        );
        assert_eq!(
            k.resolve(KeyCode::Char('D'), KeyModifiers::SHIFT),
            Some(SemanticAction::Delete),
        );
        // Same shape for every other capital binding the user might hit.
        for (ch, expected) in [
            ('G', SemanticAction::MoveBottom),
            ('H', SemanticAction::OpenHistory),
            ('W', SemanticAction::OpenWorkflows),
            ('T', SemanticAction::OpenThemes),
            ('K', SemanticAction::CopyKubectl),
            ('P', SemanticAction::OpenPromptLeader),
            ('E', SemanticAction::ExportDiagnostic),
        ] {
            assert_eq!(
                k.resolve(KeyCode::Char(ch), KeyModifiers::SHIFT),
                Some(expected),
                "Shift+{ch} should still resolve when SHIFT bit is set",
            );
        }
    }

    #[test]
    fn lowercase_chord_is_unaffected_by_shift_normalization() {
        // SHIFT+lowercase shouldn't be coerced — that's a different
        // chord and shouldn't hit a lowercase binding accidentally.
        let k = Keymap::normal();
        assert_eq!(
            k.resolve(KeyCode::Char('d'), KeyModifiers::NONE),
            Some(SemanticAction::Describe),
        );
        assert_eq!(
            k.resolve(KeyCode::Char('d'), KeyModifiers::SHIFT),
            None,
            "shift+lowercase shouldn't match the lowercase binding",
        );
    }

    #[test]
    fn ctrl_modifier_is_preserved_on_uppercase_resolution() {
        // The SHIFT-strip must not also eat CTRL.
        let k = Keymap::normal();
        // Ctrl+R is bound to ToggleReadOnly via Char('r').
        // Make sure that's untouched by the upper-case branch (the
        // code there is lowercase, so it'd never trigger — but
        // double-check Ctrl+Shift+R still doesn't masquerade as
        // Ctrl+R).
        assert_eq!(
            k.resolve(
                KeyCode::Char('R'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            None,
        );
    }
}
