//! Cross-platform clipboard helper for "copy as kubectl".

use arboard::Clipboard;

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let mut clip = Clipboard::new()?;
    clip.set_text(text.to_string())?;
    Ok(())
}
