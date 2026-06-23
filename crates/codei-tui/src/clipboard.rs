use anyhow::{Context, Result};

pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("open system clipboard")?;
    clipboard
        .set_text(text)
        .context("write to system clipboard")?;
    Ok(())
}
