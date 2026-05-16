//! Output formatters.
//!
//! Each verb produces a `Vec<T>` (or stream of `T`) and feeds it to
//! `write_records`. The text path is per-kind (different columns);
//! NDJSON / JSON / YAML are generic over any `Serialize` type.

use std::io::{self, Write};

use serde::Serialize;

use crate::args::Format;

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
