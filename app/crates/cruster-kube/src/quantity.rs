//! Parsers for the Kubernetes `Quantity` value type.
//!
//! `k8s_openapi` exposes `Quantity` as a newtype around `String` with no
//! parsing logic. The format is documented in upstream as
//! `<signedNumber><suffix>` where the suffix is one of:
//!
//! - binary SI: `Ki Mi Gi Ti Pi Ei` (powers of 1024)
//! - decimal SI: `m` (1e-3), `""` (1), `k M G T P E` (1e3 .. 1e18)
//! - decimal exponent: `e<signed>` or `E<signed>` baked into the
//!   numeric prefix (e.g. `123e9`)
//!
//! metrics-server emits CPU usage as nanocores (`123456n`) and may
//! emit microcores (`u`); both are accepted here even though they
//! aren't strictly part of the spec's decimal SI table, because real
//! payloads in the wild use them and the issue's acceptance criteria
//! call them out explicitly.

use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

/// Parse a CPU `Quantity` into cores (a unitless scalar). `1.5`
/// returns `1.5`; `500m` returns `0.5`; `123456n` returns `0.000123456`.
pub fn parse_cpu(q: &Quantity) -> Option<f64> {
    parse_quantity(&q.0)
}

/// Parse a memory `Quantity` into bytes. Fractional results round to
/// the nearest integer; negative values and NaN/infinity return
/// `None`. `1Ki` returns `1024`; `1.5Gi` returns `1610612736`.
pub fn parse_memory(q: &Quantity) -> Option<u64> {
    let v = parse_quantity(&q.0)?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some(v.round() as u64)
}

/// Lower-level parser that returns the quantity as an `f64` in
/// canonical units (cores for CPU-style, bytes for memory-style — the
/// caller decides what the unit means). Returns `None` for empty or
/// unparseable input.
pub fn parse_quantity(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num_str, suffix) = split_number_and_suffix(s)?;
    let num: f64 = num_str.parse().ok()?;
    let mult = suffix_multiplier(suffix)?;
    Some(num * mult)
}

/// Split the leading numeric token off the front of `s`. Returns
/// `(numeric_prefix, suffix)`. The numeric prefix includes an
/// optional sign, digits, optional decimal point + digits, and an
/// optional `e<signed-digits>` decimal-exponent tail. `None` if there
/// are no digits (so we don't accept e.g. `+Mi` as zero).
fn split_number_and_suffix(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;

    // Optional sign at the very start.
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    // Integer digits.
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int_len = i - int_start;

    // Optional fraction.
    let mut frac_len = 0;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        frac_len = i - frac_start;
    }

    // Must have at least one digit somewhere in mantissa.
    if int_len == 0 && frac_len == 0 {
        return None;
    }

    // Optional decimal exponent: `e`/`E` followed by signed digits.
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            // Valid exponent — fold into the numeric prefix.
            i = j;
        }
        // Else: bare `e` with no exponent digits — leave it as part
        // of the suffix so `suffix_multiplier` rejects it cleanly.
    }

    Some((&s[..i], &s[i..]))
}

/// Multiplier for a parsed SI suffix. `None` for unknown suffixes
/// (e.g. typos like `Xi`).
fn suffix_multiplier(s: &str) -> Option<f64> {
    match s {
        "" => Some(1.0),
        // Sub-unit decimal SI (and metrics-server's nano/micro).
        "n" => Some(1e-9),
        "u" => Some(1e-6),
        "m" => Some(1e-3),
        // Decimal SI (powers of 1000).
        "k" => Some(1e3),
        "M" => Some(1e6),
        "G" => Some(1e9),
        "T" => Some(1e12),
        "P" => Some(1e15),
        "E" => Some(1e18),
        // Binary SI (powers of 1024).
        "Ki" => Some(1024.0),
        "Mi" => Some(1024_f64.powi(2)),
        "Gi" => Some(1024_f64.powi(3)),
        "Ti" => Some(1024_f64.powi(4)),
        "Pi" => Some(1024_f64.powi(5)),
        "Ei" => Some(1024_f64.powi(6)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Quantity {
        Quantity(s.to_string())
    }

    // ---- parse_cpu -----------------------------------------------------

    #[test]
    fn cpu_whole_cores() {
        assert_eq!(parse_cpu(&q("1")), Some(1.0));
        assert_eq!(parse_cpu(&q("4")), Some(4.0));
    }

    #[test]
    fn cpu_decimal_cores() {
        let v = parse_cpu(&q("1.5")).unwrap();
        assert!((v - 1.5).abs() < 1e-12);
    }

    #[test]
    fn cpu_millicores() {
        let v = parse_cpu(&q("500m")).unwrap();
        assert!((v - 0.5).abs() < 1e-12);
        let v = parse_cpu(&q("250m")).unwrap();
        assert!((v - 0.25).abs() < 1e-12);
    }

    #[test]
    fn cpu_microcores() {
        let v = parse_cpu(&q("123u")).unwrap();
        assert!((v - 0.000_123).abs() < 1e-12);
    }

    #[test]
    fn cpu_nanocores_from_metrics_server() {
        // metrics-server emits CPU usage in nanocores in the wild.
        let v = parse_cpu(&q("123456789n")).unwrap();
        assert!((v - 0.123_456_789).abs() < 1e-12);
    }

    #[test]
    fn cpu_zero() {
        assert_eq!(parse_cpu(&q("0")), Some(0.0));
        assert_eq!(parse_cpu(&q("0m")), Some(0.0));
        assert_eq!(parse_cpu(&q("0n")), Some(0.0));
    }

    #[test]
    fn cpu_negative_is_returned_as_negative() {
        // Negative CPU is nonsensical for usage, but the parser is
        // unit-agnostic — let callers clamp if they want.
        let v = parse_cpu(&q("-1")).unwrap();
        assert!((v + 1.0).abs() < 1e-12);
    }

    #[test]
    fn cpu_invalid_returns_none() {
        assert_eq!(parse_cpu(&q("")), None);
        assert_eq!(parse_cpu(&q("abc")), None);
        assert_eq!(parse_cpu(&q("Mi")), None);
        assert_eq!(parse_cpu(&q("1.5Xi")), None);
    }

    // ---- parse_memory --------------------------------------------------

    #[test]
    fn memory_plain_bytes() {
        assert_eq!(parse_memory(&q("0")), Some(0));
        assert_eq!(parse_memory(&q("1024")), Some(1024));
    }

    #[test]
    fn memory_binary_si() {
        assert_eq!(parse_memory(&q("1Ki")), Some(1024));
        assert_eq!(parse_memory(&q("2Mi")), Some(2 * 1024 * 1024));
        assert_eq!(parse_memory(&q("1Gi")), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory(&q("1Ti")), Some(1024_u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn memory_decimal_si() {
        assert_eq!(parse_memory(&q("1k")), Some(1_000));
        assert_eq!(parse_memory(&q("1M")), Some(1_000_000));
        assert_eq!(parse_memory(&q("1G")), Some(1_000_000_000));
    }

    #[test]
    fn memory_decimal_exponent_form() {
        // 123e9 = 1.23e11 bytes ~= 123 GB
        let v = parse_memory(&q("123e9")).unwrap();
        assert_eq!(v, 123_000_000_000);
    }

    #[test]
    fn memory_decimal_exponent_with_explicit_sign() {
        assert_eq!(parse_memory(&q("5e+3")), Some(5_000));
        // Negative exponent → fractional bytes, rounds to nearest integer.
        assert_eq!(parse_memory(&q("5e-1")), Some(1)); // 0.5 rounds up
    }

    #[test]
    fn memory_decimal_value_rounds_to_nearest() {
        // 1.5Ki = 1536 bytes exactly.
        assert_eq!(parse_memory(&q("1.5Ki")), Some(1536));
        // 1.5Gi = 1610612736 bytes exactly.
        assert_eq!(parse_memory(&q("1.5Gi")), Some(1_610_612_736));
    }

    #[test]
    fn memory_milli_bytes_round_down() {
        // 500m bytes = 0.5 bytes → rounds to 1 (banker's nearest /
        // .round() — k8s actually rounds these up but we don't expect
        // sub-byte memory in practice).
        assert_eq!(parse_memory(&q("500m")), Some(1));
        assert_eq!(parse_memory(&q("1m")), Some(0)); // 0.001 → 0
    }

    #[test]
    fn memory_negative_returns_none() {
        assert_eq!(parse_memory(&q("-1Ki")), None);
    }

    #[test]
    fn memory_invalid_returns_none() {
        assert_eq!(parse_memory(&q("")), None);
        assert_eq!(parse_memory(&q("abc")), None);
        assert_eq!(parse_memory(&q("1Xi")), None);
    }

    #[test]
    fn memory_whitespace_is_trimmed() {
        assert_eq!(parse_memory(&q("  1Ki  ")), Some(1024));
    }

    // ---- parse_quantity edge cases ------------------------------------

    #[test]
    fn quantity_leading_decimal() {
        // ".5" should parse as 0.5.
        let v = parse_quantity(".5").unwrap();
        assert!((v - 0.5).abs() < 1e-12);
    }

    #[test]
    fn quantity_trailing_decimal() {
        // "5." should parse as 5.0.
        let v = parse_quantity("5.").unwrap();
        assert!((v - 5.0).abs() < 1e-12);
    }

    #[test]
    fn quantity_bare_e_is_not_an_exponent() {
        // `5eMi` — `e` not followed by digits → leave as suffix
        // (which then fails the multiplier lookup).
        assert_eq!(parse_quantity("5eMi"), None);
    }

    #[test]
    fn quantity_explicit_positive_sign() {
        let v = parse_quantity("+10").unwrap();
        assert!((v - 10.0).abs() < 1e-12);
    }

    #[test]
    fn quantity_just_a_sign_is_invalid() {
        assert_eq!(parse_quantity("+"), None);
        assert_eq!(parse_quantity("-"), None);
    }

    #[test]
    fn quantity_just_a_suffix_is_invalid() {
        assert_eq!(parse_quantity("Ki"), None);
        assert_eq!(parse_quantity("m"), None);
    }

    #[test]
    fn quantity_handles_all_binary_si() {
        assert_eq!(parse_quantity("1Ki"), Some(1024.0));
        assert_eq!(parse_quantity("1Mi"), Some(1024.0_f64.powi(2)));
        assert_eq!(parse_quantity("1Gi"), Some(1024.0_f64.powi(3)));
        assert_eq!(parse_quantity("1Ti"), Some(1024.0_f64.powi(4)));
        assert_eq!(parse_quantity("1Pi"), Some(1024.0_f64.powi(5)));
        assert_eq!(parse_quantity("1Ei"), Some(1024.0_f64.powi(6)));
    }

    #[test]
    fn quantity_handles_all_decimal_si() {
        assert_eq!(parse_quantity("1m"), Some(1e-3));
        assert_eq!(parse_quantity("1k"), Some(1e3));
        assert_eq!(parse_quantity("1M"), Some(1e6));
        assert_eq!(parse_quantity("1G"), Some(1e9));
        assert_eq!(parse_quantity("1T"), Some(1e12));
        assert_eq!(parse_quantity("1P"), Some(1e15));
        assert_eq!(parse_quantity("1E"), Some(1e18));
    }
}
