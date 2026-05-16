//! TUI theme: every color/style decision the renderer makes.

use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Theme {
    pub name: String,
    #[serde(default = "default_footer_bg")]
    pub footer_bg: ThemeColor,
    #[serde(default = "default_command_bg")]
    pub command_bg: ThemeColor,
    #[serde(default = "default_search_bg")]
    pub search_bg: ThemeColor,
    #[serde(default = "default_toast_bg")]
    pub toast_bg: ThemeColor,
    #[serde(default = "default_selected_bg")]
    pub selected_bg: ThemeColor,
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
}

impl ThemeColor {
    pub fn as_ratatui(&self) -> Color {
        let Self::Named(s) = self;
        match s.as_str() {
            "reset" => Color::Reset,
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "white" => Color::White,
            "gray" | "grey" => Color::Gray,
            "darkgray" | "darkgrey" => Color::DarkGray,
            "lightred" => Color::LightRed,
            "lightgreen" => Color::LightGreen,
            "lightyellow" => Color::LightYellow,
            "lightblue" => Color::LightBlue,
            "lightmagenta" => Color::LightMagenta,
            "lightcyan" => Color::LightCyan,
            hex if hex.starts_with('#') && hex.len() == 7 => parse_hex(hex).unwrap_or(Color::Reset),
            _ => Color::Reset,
        }
    }
}

fn parse_hex(s: &str) -> Option<Color> {
    let r = u8::from_str_radix(&s[1..3], 16).ok()?;
    let g = u8::from_str_radix(&s[3..5], 16).ok()?;
    let b = u8::from_str_radix(&s[5..7], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn default_footer_bg() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}
fn default_command_bg() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}
fn default_search_bg() -> ThemeColor {
    ThemeColor::Named("blue".into())
}
fn default_toast_bg() -> ThemeColor {
    ThemeColor::Named("red".into())
}
fn default_selected_bg() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}
fn default_prod() -> ThemeColor {
    ThemeColor::Named("red".into())
}
fn default_staging() -> ThemeColor {
    ThemeColor::Named("yellow".into())
}
fn default_dev() -> ThemeColor {
    ThemeColor::Named("green".into())
}
fn default_local() -> ThemeColor {
    ThemeColor::Named("cyan".into())
}
fn default_unknown() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}

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
            let t = Theme::embedded(name);
            assert!(t.is_some(), "embedded theme '{name}' missing or invalid");
            assert!(!t.unwrap().name.is_empty());
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
        assert_eq!(ThemeColor::Named("reset".into()).as_ratatui(), Color::Reset);
    }
}
