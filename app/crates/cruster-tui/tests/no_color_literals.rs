//! Integration test: views must not contain raw `Color::` literals.
//!
//! All styling must go through `Theme` so themes remain authoritative.
//! The only exception is test code, which may use `Color::` for assertions.

use std::fs;
use std::path::Path;

#[test]
fn no_color_literals_in_views() {
    let views_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/views");

    let mut violations = Vec::new();

    for entry in fs::read_dir(&views_dir).expect("views dir should exist") {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.extension().map(|e| e == "rs").unwrap_or(false) {
            check_file(&path, &mut violations);
        }
    }

    if !violations.is_empty() {
        panic!(
            "Found Color:: literals outside #[cfg(test)] blocks:\n{}",
            violations.join("\n")
        );
    }
}

fn check_file(path: &Path, violations: &mut Vec<String>) {
    let content = fs::read_to_string(path).expect("file should be readable");
    let filename = path.file_name().unwrap().to_string_lossy();

    let mut in_test_block = false;
    let mut brace_depth: usize = 0;
    let mut test_block_depth: usize = 0;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Track #[cfg(test)] mod blocks
        if trimmed.starts_with("#[cfg(test)]") {
            in_test_block = true;
            test_block_depth = brace_depth;
        }

        // Count braces to track block depth
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    if in_test_block && brace_depth <= test_block_depth {
                        in_test_block = false;
                    }
                }
                _ => {}
            }
        }

        // Check for Color:: outside test blocks
        if !in_test_block && line.contains("Color::") {
            // Skip comments
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }
            violations.push(format!("  {}:{}: {}", filename, line_num + 1, trimmed));
        }
    }
}
