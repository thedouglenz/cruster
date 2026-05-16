//! `cruster theme list|preview|install` — theme inspection + (stub) install.

use cruster_tui::theme::Theme;

use crate::args::{Cli, ThemeArgs, ThemeSub};

pub async fn run(_cli: &Cli, args: &ThemeArgs) -> anyhow::Result<()> {
    match &args.sub {
        ThemeSub::List => {
            for name in Theme::bundled_names() {
                println!("{name}");
            }
            Ok(())
        }
        ThemeSub::Preview { name } => {
            let theme =
                Theme::embedded(name).ok_or_else(|| anyhow::anyhow!("unknown theme: {name}"))?;
            // Pretty-print as JSON for easy diffing across themes.
            let json = serde_json::json!({
                "name": theme.name,
                "footer_bg": format!("{:?}", theme.footer_bg),
                "command_bg": format!("{:?}", theme.command_bg),
                "search_bg": format!("{:?}", theme.search_bg),
                "toast_bg": format!("{:?}", theme.toast_bg),
                "selected_bg": format!("{:?}", theme.selected_bg),
                "env_band": {
                    "prod": format!("{:?}", theme.env_band.prod),
                    "staging": format!("{:?}", theme.env_band.staging),
                    "dev": format!("{:?}", theme.env_band.dev),
                    "local": format!("{:?}", theme.env_band.local),
                    "unknown": format!("{:?}", theme.env_band.unknown),
                },
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
            Ok(())
        }
        ThemeSub::Install { url } => {
            anyhow::bail!("theme install is a Pro feature (not yet wired in v1). Requested: {url}");
        }
    }
}
