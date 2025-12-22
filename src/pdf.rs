use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub fn pdf_page_count(path: &Path) -> Result<Option<u32>> {
    let output = Command::new("pdfinfo").arg(path).output();
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(anyhow::Error::new(err)).context("failed to execute pdfinfo"),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Pages:") {
            let value = value.trim();
            if let Ok(pages) = value.parse::<u32>() {
                return Ok(Some(pages));
            }
        }
    }
    Ok(None)
}
