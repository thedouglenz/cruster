//! YAML edit action: write the selected resource to a temp file,
//! shell out to $EDITOR, validate the result on return, and apply
//! it back to the cluster via kubectl.
//!
//! Cruster does not embed an editor. See the design principle in the
//! product spec ("editing defers to $EDITOR").

use std::io;
use std::io::Write;
use std::process::Command;

use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

/// Open `yaml` in $EDITOR for editing. On save (editor exits 0),
/// validate the YAML and apply it via `kubectl apply -f -`. Returns
/// the new YAML string on success, or an error.
pub fn edit_and_apply(yaml: &str) -> anyhow::Result<String> {
    // Write to temp file.
    let mut path = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    path.push(format!("cruster-edit-{pid}-{nanos}.yaml"));
    {
        let mut f = std::fs::File::create(&path)?;
        f.write_all(yaml.as_bytes())?;
    }

    // Suspend the TUI.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    // Run $VISUAL → $EDITOR → vi.
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = Command::new(&editor).arg(&path).status();

    // Restore the TUI before doing anything else (even on error).
    let _ = enable_raw_mode();
    let _ = execute!(io::stdout(), EnterAlternateScreen);

    let status = status?;
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        anyhow::bail!("editor exited with status {status}; not applying");
    }

    // Read it back.
    let new_yaml = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);

    // Validate as YAML.
    let _: serde_yaml::Value = serde_yaml::from_str(&new_yaml)?;

    // Apply via kubectl. We must:
    // 1. take() stdin and drop it after writing so kubectl sees EOF
    //    (otherwise wait() hangs forever — kubectl reads stdin until
    //    EOF, and an un-dropped ChildStdin keeps the pipe open).
    // 2. capture stdout + stderr so kubectl's output doesn't paint
    //    over the alt-screen TUI when we return.
    let mut kc = Command::new("kubectl")
        .args(["apply", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    {
        let mut stdin = kc.stdin.take().expect("piped stdin");
        stdin.write_all(new_yaml.as_bytes())?;
        // stdin drops here → kubectl sees EOF.
    }
    let output = kc.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "kubectl apply exited with status {} — {}",
            output.status,
            stderr.lines().next().unwrap_or("(no stderr)")
        );
    }

    Ok(new_yaml)
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_yaml_returns_error() {
        // YAML validation runs after editor return; test the validator
        // in isolation.
        let bad: Result<serde_yaml::Value, _> = serde_yaml::from_str("a: [unclosed");
        assert!(bad.is_err());
    }

    #[test]
    fn valid_yaml_parses() {
        let good: Result<serde_yaml::Value, _> =
            serde_yaml::from_str("apiVersion: v1\nkind: Pod\nmetadata:\n  name: nginx");
        assert!(good.is_ok());
    }
}
