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
    #[serde(default = "default_selection_fg")]
    pub selection_fg: ThemeColor,
    #[serde(default = "default_header_fg")]
    pub header_fg: ThemeColor,
    #[serde(default = "default_muted_fg")]
    pub muted_fg: ThemeColor,
    /// Border color for modal overlays (delete, help, port-forward,
    /// palette, relationships, search). Defaults to a bright cyan
    /// so modals pop off the background regardless of terminal
    /// palette tuning.
    #[serde(default = "default_overlay_border")]
    pub overlay_border: ThemeColor,
    #[serde(default)]
    pub env_band: EnvBand,
    #[serde(default)]
    pub env_band_fg: EnvBandFg,
    #[serde(default)]
    pub mode: ModeColors,
    #[serde(default)]
    pub chip: ChipColors,
    #[serde(default)]
    pub status: StatusColors,
    #[serde(default)]
    pub sparkline: SparklineColors,
    #[serde(default)]
    pub gauge: GaugeColors,
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

#[derive(Debug, Clone, Deserialize)]
pub struct StatusColors {
    #[serde(default = "default_status_running")]
    pub running: ThemeColor,
    #[serde(default = "default_status_pending")]
    pub pending: ThemeColor,
    #[serde(default = "default_status_failed")]
    pub failed: ThemeColor,
    #[serde(default = "default_status_succeeded")]
    pub succeeded: ThemeColor,
    #[serde(default = "default_status_unknown")]
    pub unknown: ThemeColor,
}

impl Default for StatusColors {
    fn default() -> Self {
        Self {
            running: default_status_running(),
            pending: default_status_pending(),
            failed: default_status_failed(),
            succeeded: default_status_succeeded(),
            unknown: default_status_unknown(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SparklineColors {
    #[serde(default = "default_sparkline_primary")]
    pub primary: ThemeColor,
    #[serde(default = "default_sparkline_warn")]
    pub warn: ThemeColor,
    #[serde(default = "default_sparkline_danger")]
    pub danger: ThemeColor,
}

impl Default for SparklineColors {
    fn default() -> Self {
        Self {
            primary: default_sparkline_primary(),
            warn: default_sparkline_warn(),
            danger: default_sparkline_danger(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GaugeColors {
    #[serde(default = "default_gauge_ok")]
    pub ok: ThemeColor,
    #[serde(default = "default_gauge_warn")]
    pub warn: ThemeColor,
    #[serde(default = "default_gauge_danger")]
    pub danger: ThemeColor,
}

impl Default for GaugeColors {
    fn default() -> Self {
        Self {
            ok: default_gauge_ok(),
            warn: default_gauge_warn(),
            danger: default_gauge_danger(),
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
fn default_mode_rw() -> ThemeColor {
    ThemeColor::Named("red".into())
}
fn default_mode_ro() -> ThemeColor {
    ThemeColor::Named("green".into())
}
fn default_chip_label_fg() -> ThemeColor {
    ThemeColor::Named("cyan".into())
}
fn default_chip_value_fg() -> ThemeColor {
    ThemeColor::Named("reset".into())
}
fn default_selection_fg() -> ThemeColor {
    ThemeColor::Named("cyan".into())
}
fn default_header_fg() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}
fn default_muted_fg() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}
fn default_overlay_border() -> ThemeColor {
    ThemeColor::Named("lightcyan".into())
}
fn default_status_running() -> ThemeColor {
    ThemeColor::Named("green".into())
}
fn default_status_pending() -> ThemeColor {
    ThemeColor::Named("yellow".into())
}
fn default_status_failed() -> ThemeColor {
    ThemeColor::Named("red".into())
}
fn default_status_succeeded() -> ThemeColor {
    ThemeColor::Named("blue".into())
}
fn default_status_unknown() -> ThemeColor {
    ThemeColor::Named("darkgray".into())
}
fn default_sparkline_primary() -> ThemeColor {
    ThemeColor::Named("green".into())
}
fn default_sparkline_warn() -> ThemeColor {
    ThemeColor::Named("yellow".into())
}
fn default_sparkline_danger() -> ThemeColor {
    ThemeColor::Named("red".into())
}
fn default_gauge_ok() -> ThemeColor {
    ThemeColor::Named("green".into())
}
fn default_gauge_warn() -> ThemeColor {
    ThemeColor::Named("yellow".into())
}
fn default_gauge_danger() -> ThemeColor {
    ThemeColor::Named("red".into())
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

    #[test]
    fn chip_colors_defaults() {
        let t = Theme::terminal_default();
        assert_eq!(t.chip.label_fg.as_ratatui(), Color::Cyan);
        assert_eq!(t.chip.value_fg.as_ratatui(), Color::Reset);
    }

    #[test]
    fn mode_colors_defaults() {
        let t = Theme::terminal_default();
        assert_eq!(t.mode.rw.as_ratatui(), Color::Red);
        assert_eq!(t.mode.ro.as_ratatui(), Color::Green);
    }

    #[test]
    fn env_band_fg_defaults_pair_with_env_band_bgs() {
        let t = Theme::terminal_default();
        assert_eq!(t.env_band_fg.prod.as_ratatui(), Color::White);
        assert_eq!(t.env_band_fg.staging.as_ratatui(), Color::Black);
        assert_eq!(t.env_band_fg.dev.as_ratatui(), Color::Black);
        assert_eq!(t.env_band_fg.local.as_ratatui(), Color::Black);
        assert_eq!(t.env_band_fg.unknown.as_ratatui(), Color::White);
    }

    #[test]
    fn overlay_border_defaults_to_lightcyan_in_terminal_theme() {
        let t = Theme::terminal_default();
        assert_eq!(t.overlay_border.as_ratatui(), Color::LightCyan);
    }

    #[test]
    fn overlay_border_falls_back_to_default_when_toml_omits_key() {
        // Back-compat: a minimal TOML without `overlay_border` still
        // parses, and the missing key falls through to the default.
        let body = r#"name = "minimal""#;
        let t: Theme = toml::from_str(body).unwrap();
        assert_eq!(t.overlay_border.as_ratatui(), Color::LightCyan);
    }
}
