//! External CLI plugin manifest discovery and safe execution for beaM.

use crate::error::{BeamError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCategory {
    #[default]
    Filter,
    Export,
    Analyze,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub category: PluginCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Default user plugins directory (`~/.config/beam/plugins` or `%APPDATA%\beam\plugins`).
pub fn default_plugins_dir() -> PathBuf {
    crate::preset::default_presets_dir()
        .parent()
        .map(|p| p.join("plugins"))
        .unwrap_or_else(|| PathBuf::from("plugins"))
}

/// Discover and validate `.ron` plugin manifests inside `dir`.
pub fn discover_plugins(dir: &Path) -> Vec<PluginManifest> {
    let mut plugins = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return plugins;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ron") {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.len() > MAX_MANIFEST_BYTES {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(manifest) = ron::from_str::<PluginManifest>(&content) {
            if validate_manifest(&manifest) {
                plugins.push(manifest);
            }
        }
    }

    plugins.sort_by_key(|a| a.name.to_lowercase());
    plugins
}

fn validate_manifest(m: &PluginManifest) -> bool {
    let id = m.id.trim();
    let name = m.name.trim();
    let cmd = m.command.trim();
    if id.is_empty() || name.is_empty() || cmd.is_empty() {
        return false;
    }
    if cmd.contains("..") || cmd.contains('\0') {
        return false;
    }
    true
}

/// Write a sample CLI plugin manifest into `dir` for user customization.
pub fn install_sample_plugin(dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join("sample_exif_inspect.ron");
    let sample = PluginManifest {
        id: "sample_exif_inspect".into(),
        name: "Sample File Inspector".into(),
        description: "Logs the active photo path and sidecar path via system echo.".into(),
        command: if cfg!(windows) {
            "cmd".into()
        } else {
            "sh".into()
        },
        args: if cfg!(windows) {
            vec!["/C".into(), "echo %BEAM_IMAGE_PATH%".into()]
        } else {
            vec!["-c".into(), "echo \"$BEAM_IMAGE_PATH\"".into()]
        },
        category: PluginCategory::Analyze,
    };
    let ron_str = ron::ser::to_string_pretty(&sample, ron::ser::PrettyConfig::default())?;
    crate::fs_util::atomic_write(&path, ron_str.as_bytes())?;
    Ok(path)
}

/// Execute a validated plugin manifest against the given image and optional sidecar path.
pub fn run_plugin(
    manifest: &PluginManifest,
    image_path: &Path,
    sidecar_path: Option<&Path>,
) -> Result<PluginOutput> {
    if !validate_manifest(manifest) {
        return Err(BeamError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid plugin manifest",
        )));
    }

    let mut cmd = Command::new(manifest.command.trim());
    cmd.args(&manifest.args);
    cmd.env("BEAM_PLUGIN_ID", manifest.id.trim());
    cmd.env("BEAM_IMAGE_PATH", image_path);
    if let Some(sc) = sidecar_path {
        cmd.env("BEAM_SIDECAR_PATH", sc);
    }

    let output = cmd
        .output()
        .map_err(|e| BeamError::Io(std::io::Error::other(format!("Plugin exec failed: {e}"))))?;

    Ok(PluginOutput {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_discover_sample_and_run() {
        let dir = std::env::temp_dir().join(format!("beam_plugin_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let sample_path = install_sample_plugin(&dir).unwrap();
        assert!(sample_path.exists());

        let discovered = discover_plugins(&dir);
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].id, "sample_exif_inspect");

        let img_path = dir.join("test_shot.dng");
        let out = run_plugin(&discovered[0], &img_path, None).unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("test_shot.dng"));

        let _ = fs::remove_dir_all(&dir);
    }
}
