use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

pub(crate) fn write_text(path: &Path, content: impl AsRef<str>) -> Result<()> {
    write_bytes(path, content.as_ref().as_bytes())
}

fn write_bytes(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let mut temp = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file in {}", parent.display()))?;
    temp.write_all(content)
        .with_context(|| format!("failed to write temporary file for {}", path.display()))?;
    temp.as_file_mut()
        .flush()
        .with_context(|| format!("failed to flush temporary file for {}", path.display()))?;
    temp.as_file_mut()
        .sync_all()
        .with_context(|| format!("failed to sync temporary file for {}", path.display()))?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_text_creates_parent_directories() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("nested").join("artifact.txt");

        write_text(&path, "content\n").unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "content\n");
    }

    #[test]
    fn write_text_replaces_existing_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("artifact.txt");
        fs::write(&path, "old\n").unwrap();

        write_text(&path, "new\n").unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "new\n");
    }
}
