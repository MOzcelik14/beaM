//! Safe filesystem utilities including atomic file writes.

use crate::error::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Atomically write bytes to a file.
/// Writes to a temporary file in the same directory first, flushes and syncs,
/// then atomically renames the temporary file over the target path.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = match path.file_name() {
        Some(name) => path.with_file_name(format!("{}.{pid}_{nonce}.tmp", name.to_string_lossy())),
        None => path.with_extension("tmp"),
    };

    let write_res = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(content)?;
        file.flush()?;
        file.sync_data()?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_write_creates_and_overwrites() {
        let dir = std::env::temp_dir().join("beam_atomic_write_test");
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("file.txt");

        atomic_write(&target, b"first content").expect("Write initial");
        assert_eq!(fs::read_to_string(&target).unwrap(), "first content");

        atomic_write(&target, b"overwritten content").expect("Overwrite");
        assert_eq!(fs::read_to_string(&target).unwrap(), "overwritten content");

        let _ = fs::remove_dir_all(dir);
    }
}
