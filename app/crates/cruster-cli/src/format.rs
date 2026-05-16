//! Output formatters.
//!
//! Each verb produces a `Vec<T>` (or stream of `T`) and feeds it to
//! `write_records`. The text path is per-kind (different columns);
//! NDJSON / JSON / YAML are generic over any `Serialize` type.

use std::io::{self, Write};

use serde::Serialize;

use crate::args::Format;
use crate::budget::Budget;

/// Write `records` to `out` per the chosen format.
///
/// `text_writer` is only called when `format == Format::Text`. Pass a
/// no-op closure if your verb has no text representation.
pub fn write_records<T, W, F>(
    out: &mut W,
    format: Format,
    records: &[T],
    mut text_writer: F,
) -> io::Result<()>
where
    T: Serialize,
    W: Write,
    F: FnMut(&mut W, &[T]) -> io::Result<()>,
{
    match format {
        Format::Text => text_writer(out, records),
        Format::Ndjson => {
            for r in records {
                let mut line = serde_json::to_vec(r).expect("serialize record");
                line.push(b'\n');
                out.write_all(&line)?;
            }
            Ok(())
        }
        Format::Json => {
            let buf = serde_json::to_vec_pretty(records).expect("serialize array");
            out.write_all(&buf)?;
            out.write_all(b"\n")?;
            Ok(())
        }
        Format::Yaml => {
            let buf = serde_yaml::to_string(records).expect("serialize yaml");
            out.write_all(buf.as_bytes())?;
            Ok(())
        }
    }
}

/// NDJSON-only variant that stops emitting when `budget` would be
/// exceeded. Appends a final `{"truncated": true, "remaining": N}`
/// marker line. Pass `None` for `budget` to fall back to
/// `write_records`.
///
/// Only meaningful for streaming-shaped formats (NDJSON). For JSON
/// pretty-print or YAML the full array is emitted as-is — truncating
/// inside those formats produces invalid output.
pub fn write_records_ndjson_budgeted<T, W>(
    out: &mut W,
    records: &[T],
    budget: Option<usize>,
) -> io::Result<()>
where
    T: Serialize,
    W: Write,
{
    let Some(tokens) = budget else {
        return write_records(out, Format::Ndjson, records, |_, _| Ok(()));
    };
    let mut b = Budget::new(tokens);
    for (emitted, r) in records.iter().enumerate() {
        let mut line = serde_json::to_vec(r).expect("serialize record");
        line.push(b'\n');
        if !b.fits(line.len()) {
            let remaining = records.len() - emitted;
            let marker = serde_json::json!({"truncated": true, "remaining": remaining});
            let mut mline = serde_json::to_vec(&marker).expect("serialize marker");
            mline.push(b'\n');
            out.write_all(&mline)?;
            return Ok(());
        }
        b.consume(line.len());
        out.write_all(&line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct Row {
        name: String,
        n: i32,
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                name: "a".into(),
                n: 1,
            },
            Row {
                name: "b".into(),
                n: 2,
            },
        ]
    }

    #[test]
    fn ndjson_writes_one_line_per_record() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Ndjson, &rows(), |_, _| Ok(())).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"name\":\"a\""));
        assert!(lines[1].contains("\"name\":\"b\""));
    }

    #[test]
    fn json_writes_pretty_array() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Json, &rows(), |_, _| Ok(())).unwrap();
        let s = String::from_utf8(out).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[test]
    fn yaml_writes_yaml_array() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Yaml, &rows(), |_, _| Ok(())).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("- name: a"));
    }

    #[test]
    fn budgeted_ndjson_emits_truncated_marker() {
        let big: Vec<Row> = (0..100)
            .map(|i| Row {
                name: format!("row-with-a-fairly-long-name-{i}"),
                n: i,
            })
            .collect();
        let mut out = Vec::new();
        // Tiny budget: ~50 tokens × 4 chars = 200 chars. Each row JSON
        // is ~40 chars; we should emit ~5 rows then a marker.
        write_records_ndjson_budgeted(&mut out, &big, Some(50)).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(last["truncated"], true);
        assert!(last["remaining"].as_u64().unwrap() > 0);
        // Plus the lines that did fit.
        assert!(lines.len() > 1);
        assert!(lines.len() < 100);
    }

    #[test]
    fn budgeted_ndjson_without_budget_emits_all() {
        let rows = rows();
        let mut out = Vec::new();
        write_records_ndjson_budgeted(&mut out, &rows, None).unwrap();
        let s = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn text_uses_custom_writer() {
        let mut out = Vec::new();
        write_records(&mut out, Format::Text, &rows(), |w, rs| {
            for r in rs {
                writeln!(w, "{}\t{}", r.name, r.n)?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "a\t1\nb\t2\n");
    }
}
