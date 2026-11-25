//! External CLI plugin manifest discovery and safe execution for beaM.

use crate::error::{BeamError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_PLUGIN_OUTPUT_BYTES: usize = 1024 * 1024;
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_millis(150);

/// Defense-in-depth denylist for obvious interactive shells.
/// NOTE: This is NOT a security boundary on its own (interpreters like python/env can wrap shells);
/// the primary trust boundary is explicit user confirmation + SHA-256 manifest pinning.
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
    if cmd.contains('\0') || m.args.iter().any(|a| a.contains('\0')) {
        return false;
    }
    // Reject parent-directory traversal components when command is specified as a path.
    if Path::new(cmd)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
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

/// Compute a lowercase hex SHA-256 digest (pure Rust, FIPS 180-4) for manifest pinning.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((data.len() + 9).div_ceil(64) * 64);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for block in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let off = i * 4;
            *word =
                u32::from_be_bytes([block[off], block[off + 1], block[off + 2], block[off + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        let vals = [a, b, c, d, e, f, g, hh];
        for (acc, v) in h.iter_mut().zip(vals) {
            *acc = acc.wrapping_add(v);
        }
    }

    let mut out = String::with_capacity(64);
    for word in h {
        use std::fmt::Write;
        let _ = write!(out, "{word:08x}");
    }
    out
}

/// Compute the deterministic SHA-256 fingerprint of a `PluginManifest`.
/// Any change to `id`, `name`, `command`, `args`, or `category` produces a new hash
/// and invalidates prior user approval.
pub fn manifest_sha256(m: &PluginManifest) -> String {
    let mut canonical = String::new();
    canonical.push_str(m.id.trim());
    canonical.push('\0');
    canonical.push_str(m.name.trim());
    canonical.push('\0');
    canonical.push_str(m.command.trim());
    canonical.push('\0');
    canonical.push_str(&format!("{:?}", m.category));
    for arg in &m.args {
        canonical.push('\0');
        canonical.push_str(arg);
    }
    sha256_hex(canonical.as_bytes())
}

/// Resolve a plugin command to a concrete executable path (either explicit or via `PATH`)
/// for display in the user confirmation prompt.
pub fn resolve_command_path(command: &str) -> Option<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    if trimmed.contains('/') || trimmed.contains('\\') {
        return p.is_file().then(|| p.to_path_buf());
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(trimmed);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe_candidate = dir.join(format!("{trimmed}.exe"));
            if exe_candidate.is_file() {
                return Some(exe_candidate);
            }
        }
    }
    None
}

/// Check whether `manifest` matches the user-approved SHA-256 hash stored for its `id`.
pub fn is_manifest_trusted(
    manifest: &PluginManifest,
    approved_hashes: &BTreeMap<String, String>,
) -> bool {
    let expected = manifest_sha256(manifest);
    approved_hashes.get(manifest.id.trim()) == Some(&expected)
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

fn kill_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // Send SIGKILL to the entire process group (-pgid) created via process_group(0).
        let _ = Command::new("kill")
            .args(["-s", "KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        // Terminate child and all descendant processes on Windows.
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Execute a plugin only if its manifest SHA-256 has been explicitly approved by the user.
pub fn run_plugin_trusted(
    manifest: &PluginManifest,
    approved_hashes: &BTreeMap<String, String>,
    image_path: &Path,
    sidecar_path: Option<&Path>,
) -> Result<PluginOutput> {
    if !is_manifest_trusted(manifest, approved_hashes) {
        return Err(BeamError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "Plugin '{}' is not approved or its manifest has changed",
                manifest.id
            ),
        )));
    }
    run_plugin(manifest, image_path, sidecar_path)
}

/// Execute a validated plugin manifest against the given image and optional sidecar path
/// with a strict 30-second timeout and 1 MB stdout/stderr cap.
pub fn run_plugin(
    manifest: &PluginManifest,
    image_path: &Path,
    sidecar_path: Option<&Path>,
) -> Result<PluginOutput> {
    run_plugin_with_timeout(manifest, image_path, sidecar_path, PLUGIN_TIMEOUT)
}

/// Execute a validated plugin manifest with a custom timeout.
/// Spawns the child in its own process group (Unix) / terminates the process tree (Windows)
/// on timeout, and drains pipes via bounded `recv_timeout` so descendant processes cannot hang.
pub fn run_plugin_with_timeout(
    manifest: &PluginManifest,
    image_path: &Path,
    sidecar_path: Option<&Path>,
    timeout: Duration,
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

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| BeamError::Io(std::io::Error::other(format!("Plugin exec failed: {e}"))))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();

    std::thread::spawn(move || {
        let data = stdout_pipe
            .map(|r| read_capped(r, MAX_PLUGIN_OUTPUT_BYTES))
            .unwrap_or_default();
        let _ = stdout_tx.send(data);
    });
    std::thread::spawn(move || {
        let data = stderr_pipe
            .map(|r| read_capped(r, MAX_PLUGIN_OUTPUT_BYTES))
            .unwrap_or_default();
        let _ = stderr_tx.send(data);
    });

    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if start.elapsed() >= timeout {
            kill_process_tree(&mut child);
            return Err(BeamError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Plugin execution timed out after {}s", timeout.as_secs()),
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let stdout_bytes = stdout_rx
        .recv_timeout(PIPE_DRAIN_TIMEOUT)
        .unwrap_or_default();
    let stderr_bytes = stderr_rx
        .recv_timeout(PIPE_DRAIN_TIMEOUT)
        .unwrap_or_default();

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

    #[test]
    fn test_sha256_nist_vector_and_manifest_pinning() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let mut m = PluginManifest {
            id: "escape_probe".into(),
            name: "Escape Probe".into(),
            description: String::new(),
            command: "env".into(),
            args: vec!["sh".into(), "-c".into(), "id".into()],
            category: PluginCategory::Analyze,
        };
        let mut approved = BTreeMap::new();

        // Untrusted interpreter wrapper must be blocked before spawning
        let err = run_plugin_trusted(&m, &approved, Path::new("photo.dng"), None)
            .expect_err("Untrusted manifest must be rejected");
        assert!(err.to_string().contains("not approved"));

        // Approving pins the current SHA-256
        approved.insert(m.id.clone(), manifest_sha256(&m));
        assert!(is_manifest_trusted(&m, &approved));

        // Modifying args invalidates prior approval
        m.args.push("; uname -a".into());
        assert!(!is_manifest_trusted(&m, &approved));
    }

    #[cfg(unix)]
    #[test]
    fn test_plugin_timeout_kills_descendant_holding_pipe() {
        // Spawn a Python process that forks a grandchild sleeping 30s with stdout inherited.
        // Without process-group kill + bounded channel drain, this would block for 30s.
        let pid_file =
            std::env::temp_dir().join(format!("beam_grandchild_pid_{}", std::process::id()));
        let _ = fs::remove_file(&pid_file);
        let script = format!(
            "import subprocess, time; p = subprocess.Popen(['sleep', '30']); open({:?}, 'w').write(str(p.pid)); time.sleep(30)",
            pid_file.to_string_lossy()
        );
        let m = PluginManifest {
            id: "hang_test".into(),
            name: "Hang Test".into(),
            description: String::new(),
            command: "python3".into(),
            args: vec!["-c".into(), script],
            category: PluginCategory::Analyze,
        };

        let start = Instant::now();
        let res =
            run_plugin_with_timeout(&m, Path::new("photo.dng"), None, Duration::from_millis(350));
        let elapsed = start.elapsed();

        assert!(res.is_err(), "Timed-out plugin must return Err");
        assert!(
            elapsed < Duration::from_secs(3),
            "Timeout took too long ({elapsed:?}), pipe/descendant likely blocked!"
        );

        // Verify the descendant process was also killed by killpg (not running; zombie 'Z' is dead)
        if let Ok(pid_str) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                std::thread::sleep(Duration::from_millis(100));
                let is_zombie = fs::read_to_string(format!("/proc/{pid}/stat"))
                    .map(|s| s.contains(") Z "))
                    .unwrap_or(false);
                let alive = !is_zombie
                    && Command::new("kill")
                        .args(["-0", &pid.to_string()])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                assert!(
                    !alive,
                    "Grandchild process {pid} survived process-group kill!"
                );
            }
        }
        let _ = fs::remove_file(&pid_file);
    }
}
