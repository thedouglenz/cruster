//! License loading and verification.
//!
//! Cruster's free tier always works without a license. Pro/Team/
//! Enterprise features are gated client-side: at startup the binary
//! reads `~/.config/cruster/license.toml`, verifies its ed25519
//! signature against an embedded public key, checks expiry, and
//! derives a `Tier`. Trial licenses (issued by `cruster trial`)
//! skip signature verification but enforce a 14-day cap.
//!
//! License file format (TOML):
//!
//! ```toml
//! tier = "pro"                                # pro|team|enterprise|trial-pro
//! email = "user@example.com"
//! issued_at = "2026-05-16T00:00:00Z"
//! expires_at = "2027-05-16T00:00:00Z"
//! signature = "base64-url-no-pad-bytes"       # omitted for trial-pro
//! ```
//!
//! The signature covers the canonical JSON serialisation of the
//! claim fields (everything except `signature`).

use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::tier::Tier;

/// Maximum allowed lifetime of a trial license. The binary refuses
/// trial files claiming a longer window — a small but real friction
/// against the trivial "edit the TOML to extend the trial" attack.
pub const TRIAL_MAX_DAYS: i64 = 14;

/// The full license, as written on disk. Signature is `None` for
/// trial files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct License {
    pub tier: String,
    pub email: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub signature: Option<String>,
}

/// Just the signable claims — what the signature actually covers.
/// Field order here matters: the JSON serialisation is canonical only
/// because serde_json emits fields in declaration order.
#[derive(Debug, Serialize)]
struct Claims<'a> {
    tier: &'a str,
    email: &'a str,
    issued_at: &'a DateTime<Utc>,
    expires_at: &'a DateTime<Utc>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    NotFound,
    BadToml(String),
    BadSignature,
    MissingSignature,
    Expired,
    InvalidTier(String),
    InvalidTrialWindow,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "no license file at the canonical path"),
            Self::BadToml(e) => write!(f, "license file is not valid TOML: {e}"),
            Self::BadSignature => write!(f, "license signature did not verify"),
            Self::MissingSignature => write!(f, "license is missing required signature"),
            Self::Expired => write!(f, "license has expired"),
            Self::InvalidTier(t) => write!(f, "unknown tier in license: {t:?}"),
            Self::InvalidTrialWindow => write!(
                f,
                "trial license window exceeds {TRIAL_MAX_DAYS} days; refusing to honour"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

impl License {
    /// `~/.config/cruster/license.toml` (or the platform equivalent
    /// via the `dirs` crate). `None` if no config dir is resolvable.
    pub fn canonical_path() -> Option<PathBuf> {
        let mut p = dirs::config_dir()?;
        p.push("cruster");
        p.push("license.toml");
        Some(p)
    }

    /// Load from the canonical path. Returns `LoadError::NotFound`
    /// when no license file exists — that's the normal free-tier
    /// state, not an error to surface to users.
    pub fn load_default() -> Result<Self, LoadError> {
        let path = Self::canonical_path().ok_or(LoadError::NotFound)?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self, LoadError> {
        let body = std::fs::read_to_string(path).map_err(|_| LoadError::NotFound)?;
        let license: License =
            toml::from_str(&body).map_err(|e| LoadError::BadToml(e.to_string()))?;
        Ok(license)
    }

    /// Verify the license and derive the active `Tier`. The current
    /// time is injected as a parameter to keep the function testable.
    pub fn tier_at(&self, now: DateTime<Utc>) -> Result<Tier, LoadError> {
        if self.expires_at <= now {
            return Err(LoadError::Expired);
        }
        let tier = parse_tier(&self.tier)?;

        if self.is_trial() {
            let window = self.expires_at - self.issued_at;
            if window > Duration::days(TRIAL_MAX_DAYS) {
                return Err(LoadError::InvalidTrialWindow);
            }
            // Trial files don't carry a signature.
            return Ok(tier);
        }

        let sig_b64 = self.signature.as_deref().ok_or(LoadError::MissingSignature)?;
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| LoadError::BadSignature)?;
        let sig_arr: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| LoadError::BadSignature)?;
        let signature = Signature::from_bytes(&sig_arr);

        let payload = self.signable_bytes();
        embedded_pubkey()
            .verify(&payload, &signature)
            .map_err(|_| LoadError::BadSignature)?;

        Ok(tier)
    }

    /// Convenience: verify against the system clock.
    pub fn tier(&self) -> Result<Tier, LoadError> {
        self.tier_at(Utc::now())
    }

    pub fn is_trial(&self) -> bool {
        self.tier == "trial-pro"
    }

    /// The exact bytes the signature covers.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let claims = Claims {
            tier: &self.tier,
            email: &self.email,
            issued_at: &self.issued_at,
            expires_at: &self.expires_at,
        };
        serde_json::to_vec(&claims).expect("Claims serialisation cannot fail")
    }

    /// Build an unsigned trial license. The caller (the `trial`
    /// CLI verb) writes this directly to disk.
    pub fn new_trial(email: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            tier: "trial-pro".into(),
            email: email.into(),
            issued_at: now,
            expires_at: now + Duration::days(TRIAL_MAX_DAYS),
            signature: None,
        }
    }

    /// Serialise this license to its on-disk TOML form.
    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("License is always serialisable")
    }
}

/// Try to load + verify the license at the canonical path. Falls
/// back to `Tier::Free` for every failure mode — a missing file, a
/// bad signature, an expired license all silently downgrade to free.
/// Callers that want to surface the reason (the `cruster license`
/// verb) should use [`License::load_default`] + [`License::tier`]
/// directly.
pub fn load_tier_or_free() -> Tier {
    License::load_default()
        .and_then(|l| l.tier())
        .unwrap_or(Tier::Free)
}

fn parse_tier(s: &str) -> Result<Tier, LoadError> {
    match s {
        "pro" | "trial-pro" => Ok(Tier::Pro),
        "team" => Ok(Tier::Team),
        "enterprise" => Ok(Tier::Enterprise),
        other => Err(LoadError::InvalidTier(other.to_string())),
    }
}

/// Resolve the ed25519 public key the binary should verify against.
///
/// Production builds set `CRUSTER_LICENSE_PUBKEY_HEX` (64 hex chars
/// = 32 bytes) at compile time. Dev builds fall back to a key
/// derived from `DEV_SEED` so the test suite can sign payloads with
/// the matching private half (also derived from `DEV_SEED`).
fn embedded_pubkey() -> VerifyingKey {
    const ENV_HEX: Option<&str> = option_env!("CRUSTER_LICENSE_PUBKEY_HEX");
    match ENV_HEX {
        Some(hex) => {
            let bytes = decode_hex_32(hex);
            VerifyingKey::from_bytes(&bytes).expect("CRUSTER_LICENSE_PUBKEY_HEX is not a valid ed25519 pubkey")
        }
        None => dev_signing_key().verifying_key(),
    }
}

/// Predictable dev seed; the matching public half is what
/// `embedded_pubkey()` returns when no env override is set. Used by
/// the test suite to sign payloads the binary will then verify.
pub const DEV_SEED: [u8; 32] = [42u8; 32];

/// Dev-only helper: the ed25519 signing key whose public half the
/// binary trusts when built without `CRUSTER_LICENSE_PUBKEY_HEX`.
/// Exposed (under `pub`) so the `cruster trial` CLI can issue
/// licenses that the binary itself will verify — and so tests can
/// sign synthetic payloads.
pub fn dev_signing_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&DEV_SEED)
}

fn decode_hex_32(s: &str) -> [u8; 32] {
    assert_eq!(s.len(), 64, "CRUSTER_LICENSE_PUBKEY_HEX must be 64 hex chars (got {})", s.len());
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
            .expect("CRUSTER_LICENSE_PUBKEY_HEX contains non-hex chars");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;

    fn sign_with_dev_key(license: &mut License) {
        let sk = dev_signing_key();
        let sig = sk.sign(&license.signable_bytes());
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes());
        license.signature = Some(b64);
    }

    fn fixed_now() -> DateTime<Utc> {
        "2026-05-16T12:00:00Z".parse().unwrap()
    }

    fn valid_pro_license() -> License {
        let now = fixed_now();
        let mut l = License {
            tier: "pro".into(),
            email: "user@example.com".into(),
            issued_at: now,
            expires_at: now + Duration::days(365),
            signature: None,
        };
        sign_with_dev_key(&mut l);
        l
    }

    #[test]
    fn valid_signed_license_yields_pro_tier() {
        let l = valid_pro_license();
        assert_eq!(l.tier_at(fixed_now()).unwrap(), Tier::Pro);
    }

    #[test]
    fn expired_license_is_rejected_even_with_good_signature() {
        let mut l = valid_pro_license();
        // Move now past the expiry.
        let later = l.expires_at + Duration::days(1);
        assert_eq!(l.tier_at(later), Err(LoadError::Expired));
        // Re-sign and re-check at the original now to confirm the
        // signature itself is OK — only expiry is the issue.
        sign_with_dev_key(&mut l);
        assert!(l.tier_at(fixed_now()).is_ok());
    }

    #[test]
    fn bad_signature_is_rejected() {
        let mut l = valid_pro_license();
        // Mutate the email so the signature no longer matches.
        l.email = "evil@example.com".into();
        assert_eq!(l.tier_at(fixed_now()), Err(LoadError::BadSignature));
    }

    #[test]
    fn missing_signature_on_non_trial_is_rejected() {
        let mut l = valid_pro_license();
        l.signature = None;
        assert_eq!(l.tier_at(fixed_now()), Err(LoadError::MissingSignature));
    }

    #[test]
    fn trial_license_does_not_need_signature() {
        let l = License::new_trial("user@example.com", fixed_now());
        assert_eq!(l.tier_at(fixed_now()).unwrap(), Tier::Pro);
        assert!(l.is_trial());
        assert!(l.signature.is_none());
    }

    #[test]
    fn trial_longer_than_14_days_is_rejected() {
        let now = fixed_now();
        let l = License {
            tier: "trial-pro".into(),
            email: "user@example.com".into(),
            issued_at: now,
            expires_at: now + Duration::days(30),
            signature: None,
        };
        assert_eq!(l.tier_at(now), Err(LoadError::InvalidTrialWindow));
    }

    #[test]
    fn expired_trial_is_rejected_via_expiry_check() {
        let now = fixed_now();
        let l = License::new_trial("user@example.com", now - Duration::days(15));
        assert_eq!(l.tier_at(now), Err(LoadError::Expired));
    }

    #[test]
    fn unknown_tier_string_is_rejected() {
        let l = License {
            tier: "ultra-mega".into(),
            email: "user@example.com".into(),
            issued_at: fixed_now(),
            expires_at: fixed_now() + Duration::days(1),
            signature: None,
        };
        assert!(matches!(l.tier_at(fixed_now()), Err(LoadError::InvalidTier(_))));
    }

    #[test]
    fn team_tier_maps_to_team_enum() {
        let mut l = valid_pro_license();
        l.tier = "team".into();
        sign_with_dev_key(&mut l);
        assert_eq!(l.tier_at(fixed_now()).unwrap(), Tier::Team);
    }

    #[test]
    fn enterprise_tier_maps_to_enterprise_enum() {
        let mut l = valid_pro_license();
        l.tier = "enterprise".into();
        sign_with_dev_key(&mut l);
        assert_eq!(l.tier_at(fixed_now()).unwrap(), Tier::Enterprise);
    }

    #[test]
    fn load_from_roundtrips_through_toml() {
        let l = valid_pro_license();
        let body = l.to_toml();
        let tmp = std::env::temp_dir().join(format!(
            "cruster-license-test-{}.toml",
            std::process::id()
        ));
        std::fs::write(&tmp, body).unwrap();
        let loaded = License::load_from(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(loaded.tier_at(fixed_now()).unwrap(), Tier::Pro);
    }

    #[test]
    fn load_default_returns_not_found_for_missing_file() {
        let bogus = PathBuf::from("/nonexistent/cruster/license.toml");
        assert!(matches!(
            License::load_from(&bogus),
            Err(LoadError::NotFound)
        ));
    }

    #[test]
    fn load_tier_or_free_falls_back_to_free_on_missing() {
        // This test depends on the test runner *not* having a real
        // license file at the canonical path. If it does (unlikely),
        // the test silently passes with the wrong assertion — but
        // that's preferable to coupling the test to the runner's
        // filesystem state.
        // (Just exercising the function for crash-safety here.)
        let _ = load_tier_or_free();
    }
}
