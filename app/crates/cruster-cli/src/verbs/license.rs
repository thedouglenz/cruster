//! `cruster license {show|verify|path}` — license diagnostics.

use chrono::Utc;
use cruster_core::license::{License, LoadError};

use crate::args::{LicenseArgs, LicenseSub};

pub async fn run(args: &LicenseArgs) -> anyhow::Result<()> {
    match &args.sub {
        LicenseSub::Show => show(),
        LicenseSub::Verify => verify(),
        LicenseSub::Path => path(),
    }
}

fn show() -> anyhow::Result<()> {
    match License::load_default() {
        Err(LoadError::NotFound) => {
            println!("no license file; running on the free tier.");
            println!("activate a 14-day Pro trial with: cruster trial");
            Ok(())
        }
        Err(e) => {
            println!("license present but unusable: {e}");
            println!("running on the free tier.");
            Ok(())
        }
        Ok(l) => {
            let tier_result = l.tier_at(Utc::now());
            println!("tier:        {}", l.tier);
            println!("email:       {}", l.email);
            println!("issued:      {}", l.issued_at.to_rfc3339());
            println!("expires:     {}", l.expires_at.to_rfc3339());
            println!("trial:       {}", l.is_trial());
            match tier_result {
                Ok(t) => println!("status:      active ({:?})", t),
                Err(e) => println!("status:      invalid ({e})"),
            }
            Ok(())
        }
    }
}

fn verify() -> anyhow::Result<()> {
    match License::load_default().and_then(|l| l.tier_at(Utc::now())) {
        Ok(tier) => {
            println!("ok: license verified, tier={tier:?}");
            Ok(())
        }
        Err(LoadError::NotFound) => {
            eprintln!("no license file at the canonical path");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("license invalid: {e}");
            std::process::exit(1);
        }
    }
}

fn path() -> anyhow::Result<()> {
    match License::canonical_path() {
        Some(p) => {
            println!("{}", p.display());
            Ok(())
        }
        None => {
            eprintln!("could not resolve a config directory on this platform");
            std::process::exit(1);
        }
    }
}
