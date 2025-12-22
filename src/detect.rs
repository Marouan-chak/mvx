use std::path::Path;

#[derive(Debug, Clone)]
pub struct DetectedType {
    pub mime: Option<String>,
    pub ext_hint: Option<String>,
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

    DetectedType { mime, ext_hint }
}
