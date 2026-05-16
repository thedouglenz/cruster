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
        m.insert((KeyCode::Char('r'), none), SemanticAction::OpenRelationships);
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
}
