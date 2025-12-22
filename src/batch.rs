use anyhow::{Context, Result, bail};
use glob::glob;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct BatchInput {
    pub dest_dir: PathBuf,
    pub to_ext: Option<String>,
}

pub fn collect_sources(
    sources: &[String],
    stdin_sources: Vec<String>,
    recursive: bool,
) -> Result<Vec<PathBuf>> {
    let mut paths = BTreeSet::new();
    for input in sources.iter().chain(stdin_sources.iter()) {
        if looks_like_glob(input) {
            for path in glob(input).context("invalid glob pattern")?.flatten() {
                add_path(&mut paths, &path, recursive)?;
            }
            continue;
        }
        add_path(&mut paths, &PathBuf::from(input), recursive)?;
    }
    Ok(paths.into_iter().collect())
}

pub fn dest_for_source(input: &BatchInput, source: &Path) -> Result<PathBuf> {
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .context("source must have a file name")?;
    if let Some(ext) = input.to_ext.as_deref() {
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .context("source must have a file stem")?;
        let sanitized = ext.trim_start_matches('.');
        return Ok(input.dest_dir.join(format!("{stem}.{}", sanitized)));
    }
    Ok(input.dest_dir.join(file_name))
}

fn add_path(paths: &mut BTreeSet<PathBuf>, path: &Path, recursive: bool) -> Result<()> {
    if path.is_dir() {
        if recursive {
            for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
                if entry.file_type().is_file() {
                    paths.insert(entry.path().to_path_buf());
                }
            }
        } else {
            for entry in std::fs::read_dir(path).context("read directory")? {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path.is_file() {
                    paths.insert(entry_path);
                }
            }
        }
        return Ok(());
    }
    if path.exists() {
        paths.insert(path.to_path_buf());
        return Ok(());
    }
    if looks_like_glob(path.to_string_lossy().as_ref()) {
        return Ok(());
    }
    bail!("input not found: {}", path.display());
}

fn looks_like_glob(input: &str) -> bool {
    input.contains('*') || input.contains('?') || input.contains('[')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn dest_with_extension_override() {
        let input = BatchInput {
            dest_dir: PathBuf::from("/tmp/out"),
            to_ext: Some("mp3".to_string()),
        };
        let dest = dest_for_source(&input, Path::new("clip.wav")).unwrap();
        assert_eq!(dest, PathBuf::from("/tmp/out/clip.mp3"));
    }

    #[test]
    fn collect_sources_from_dir() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        let sources =
            collect_sources(&[dir.to_string_lossy().to_string()], Vec::new(), false).unwrap();
        assert_eq!(sources.len(), 2);
    }
}
