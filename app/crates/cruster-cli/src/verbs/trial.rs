//! `cruster trial` — activate a 14-day Pro trial.
//!
//! Writes an unsigned trial license to the canonical path. Refuses
//! to overwrite an existing file so a real (paid) license isn't
//! accidentally clobbered.

use chrono::Utc;
use cruster_core::license::License;

pub async fn run() -> anyhow::Result<()> {
    let path = License::canonical_path()
        .ok_or_else(|| anyhow::anyhow!("could not resolve a config directory on this platform"))?;

    if path.exists() {
        anyhow::bail!(
            "license file already exists at {}; refusing to overwrite.\n\
             run `cruster license show` to inspect, or delete the file and re-run `cruster trial`.",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let license = License::new_trial("trial@cruster.dev", Utc::now());
    std::fs::write(&path, license.to_toml())?;

    println!(
        "trial activated; expires {}.",
        license.expires_at.format("%Y-%m-%d")
    );
    println!("thanks for trying cruster. write to dev@cruster.dev when you're ready to upgrade.");
    Ok(())
}
