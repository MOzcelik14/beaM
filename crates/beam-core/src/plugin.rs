//! External CLI plugin manifest discovery and safe execution for beaM.

use crate::error::{BeamError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_PLUGIN_OUTPUT_BYTES: usize = 1024 * 1024;
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);

const DISALLOWED_SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
];

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

    let ron_opts = ron::Options::default().with_recursion_limit(16);
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
        if let Ok(manifest) = ron_opts.from_str::<PluginManifest>(&content) {
            if validate_manifest(&manifest) {
                plugins.push(manifest);
            }
        }
    }

    plugins.sort_by_key(|a| a.name.to_lowercase());
    plugins
}

pub fn validate_manifest(m: &PluginManifest) -> bool {
    let id = m.id.trim();
    let name = m.name.trim();
    let cmd = m.command.trim();
    if id.is_empty() || name.is_empty() || cmd.is_empty() {
        return false;
    }
    if cmd.contains("..") || cmd.contains('\0') {
        return false;
    }
    if m.args.iter().any(|a| a.contains('\0')) {
        return false;
    }
    let exe_name = Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
        .to_ascii_lowercase();
    if DISALLOWED_SHELLS.contains(&exe_name.as_str()) {
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
        description: "Runs a direct non-shell inspection command with BEAM_IMAGE_PATH set.".into(),
        command: if cfg!(windows) {
            "hostname".into()
        } else {
            "printenv".into()
        },
        args: if cfg!(windows) {
            Vec::new()
        } else {
            vec!["BEAM_IMAGE_PATH".into()]
        },
        category: PluginCategory::Analyze,
    };
    let ron_str = ron::ser::to_string_pretty(&sample, ron::ser::PrettyConfig::default())?;
    crate::fs_util::atomic_write(&path, ron_str.as_bytes())?;
    Ok(path)
}

fn read_capped<R: Read>(mut reader: R, limit: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = reader.by_ref().take(limit as u64).read_to_end(&mut buf);
    buf
}

/// Execute a validated plugin manifest against the given image and optional sidecar path
/// with a strict 30-second timeout and 1 MB stdout/stderr cap.
pub fn run_plugin(
    manifest: &PluginManifest,
    image_path: &Path,
    sidecar_path: Option<&Path>,
) -> Result<PluginOutput> {
    if !validate_manifest(manifest) {
        return Err(BeamError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid or disallowed plugin manifest",
        )));
    }

    let mut cmd = Command::new(manifest.command.trim());
    cmd.args(&manifest.args);
    cmd.env("BEAM_PLUGIN_ID", manifest.id.trim());
    cmd.env("BEAM_IMAGE_PATH", image_path);
    if let Some(sc) = sidecar_path {
        cmd.env("BEAM_SIDECAR_PATH", sc);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| BeamError::Io(std::io::Error::other(format!("Plugin exec failed: {e}"))))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        stdout_pipe
            .map(|r| read_capped(r, MAX_PLUGIN_OUTPUT_BYTES))
            .unwrap_or_default()
    });
    let stderr_handle = std::thread::spawn(move || {
        stderr_pipe
            .map(|r| read_capped(r, MAX_PLUGIN_OUTPUT_BYTES))
            .unwrap_or_default()
    });

    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if start.elapsed() >= PLUGIN_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            return Err(BeamError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Plugin execution timed out after 30s",
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    Ok(PluginOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout_bytes).trim().to_string(),
        stderr: String::from_utf8_lossy(&stderr_bytes).trim().to_string(),
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
        if !cfg!(windows) {
            assert!(out.stdout.contains("test_shot.dng"));
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_plugin_rejects_shell_wrappers() {
        for shell in [
            "sh",
            "/bin/bash",
            "cmd",
            "cmd.exe",
            "powershell.exe",
            "pwsh",
        ] {
            let m = PluginManifest {
                id: "bad".into(),
                name: "Bad".into(),
                description: String::new(),
                command: shell.into(),
                args: vec!["-c".into(), "id".into()],
                category: PluginCategory::Analyze,
            };
            assert!(!validate_manifest(&m), "Shell {shell} should be rejected");
        }
    }
}
