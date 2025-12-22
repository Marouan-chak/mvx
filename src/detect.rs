use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DetectedType {
    pub mime: Option<String>,
    pub ext_hint: Option<String>,
    pub file_mime: Option<String>,
}

pub fn detect_path(path: &Path) -> DetectedType {
    let mime = infer::get_from_path(path)
        .ok()
        .flatten()
        .map(|kind| kind.mime_type().to_string());
    let ext_hint = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    let file_mime = detect_file_mime(path);

    DetectedType {
        mime,
        ext_hint,
        file_mime,
    }
}

fn detect_file_mime(path: &Path) -> Option<String> {
    let output = Command::new("file")
        .arg("--mime-type")
        .arg("-b")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout);
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
