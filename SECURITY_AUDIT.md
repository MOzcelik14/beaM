# beaM. Security and Robustness Audit Report

**Date:** 2026-11-01  
**Target:** beaM. v0.1.1  
**Repository:** https://github.com/MOzcelik14/beaM.git  
**License:** GNU General Public License v3.0 (GPL-3.0-only)  
**Rust Edition:** 2021  
**Auditor:** Automated and Manual Security Verification

---

## 1. Executive Summary

A comprehensive security, safety, and robustness audit was performed on **beaM.**, a non-destructive RAW photo editor and library manager written in Rust using `egui`/`eframe`, `wgpu`/`WGSL`, `rawler`, `image`, `moxcms`, `kamadak-exif`, and `rusqlite`.

The audit evaluated:
1. Automated dependency audits and supply chain integrity (`cargo-audit`, `cargo-deny`).
2. Robustness against decompression bombs, malformed image files, and resource exhaustion.
3. Path traversal vulnerabilities during batch export and sidecar operations.
4. Input validation and serialization boundaries for RON sidecars, presets, and configuration files.
5. Directory scanning safety (symlink loop detection, recursion limits).
6. SQLite database integrity and schema version enforcement.
7. Concurrency stability and panic containment across background worker threads.
8. GPU pipeline robustness (zero-dimension textures, 32-bit allocation integer overflows).
9. Memory safety and static analysis (zero `unsafe` code blocks, zero `transmute` calls).
10. Property-based testing (`proptest`) and coverage-guided fuzz testing (`cargo-fuzz` / libFuzzer with AddressSanitizer).

A total of 12 security and robustness findings (BEAM-SEC-01 through BEAM-SEC-12) were identified, systematically remediated, and verified with regression tests. All 74 unit, integration, and property tests pass in debug and release mode (with runtime overflow checks enabled). 6 coverage-guided fuzz targets executed over 4,021,814 total fuzz iterations under AddressSanitizer with zero crashes, panics, or memory violations.

---

## 2. Vulnerability Findings and Remediation Matrix

| Finding ID | Severity | Status | Component | Vulnerability / Weakness | Remediation Commit |
|---|---|---|---|---|---|
| BEAM-SEC-01 | High | Fixed | `beam-media` | Unbounded image decompression allocation (Decompression Bomb / OOM Denial of Service) | `3dd552f` |
| BEAM-SEC-02 | Medium | Fixed | `beam-media` | Zero-dimension divide-by-zero panic in proxy generation and raster orientation transforms | `3dd552f` |
| BEAM-SEC-03 | Medium | Fixed | `beam-library` | Infinite loop and stack overflow risk on recursive directory scan with circular symlinks | `a83b97e` |
| BEAM-SEC-04 | Medium | Fixed | `beam-core` | Unvalidated NaN, Inf, and out-of-bounds parameters in RON sidecars and presets | `b291881` |
| BEAM-SEC-05 | High | Fixed | `beam-media` | Path traversal and arbitrary file write via unsanitized filename export templates | `49e10c5` |
| BEAM-SEC-06 | Medium | Fixed | `beam-core`, `beam-media` | Non-atomic sidecar and thumbnail file writes prone to data corruption on unexpected crash or power loss | `b291881` |
| BEAM-SEC-07 | Medium | Fixed | `beam-app` | Uncaught panics in background worker thread causing UI freeze or application lockup | `c0da755` |
| BEAM-SEC-08 | Medium | Fixed | `beam-app` | Unbounded in-memory thumbnail texture cache leading to memory exhaustion | `c0da755` |
| BEAM-SEC-09 | Low | Fixed | `beam-library` | Missing SQLite schema version check allowing database corruption by newer/incompatible schemas | `a83b97e` |
| BEAM-SEC-10 | Low | Fixed | Root | Missing GPL-3.0 LICENSE file, unpinned dependencies in Cargo.lock, missing cargo-deny policy | `a6448de` |
| BEAM-SEC-11 | Medium | Fixed | `beam-gpu` | 32-bit integer overflow in staging buffer size calculation and zero-dimension texture panics | `1db83ff` |
| BEAM-SEC-12 | Medium | Fixed | `beam-media` | Unbounded embedded ICC color profile memory allocation | `3dd552f` |

---

## 3. Detailed Vulnerability Analyses and Fixes

### BEAM-SEC-01: Unbounded Image Decompression Allocation (High)
- **Problem:** `beam-media/src/decoder.rs` used `image::ImageReader::open` without limits, making the application vulnerable to decompression bombs (zip bombs / pixel bombs). A small file (e.g., highly compressed PNG/TIFF) could declare 100,000 x 100,000 dimensions, triggering allocation failure or OOM crash.
- **Fix:** Implemented strict limits in `beam-media`:
  - `MAX_IMAGE_FILE_SIZE = 500 * 1024 * 1024` (500 MB)
  - `MAX_IMAGE_DIMENSION = 32_768` pixels per dimension
  - `MAX_IMAGE_PIXELS = 150_000_000` pixels total
  - `MAX_IMAGE_DECODE_ALLOC = 1024 * 1024 * 1024` (1 GB allocation cap)
  - Configured `image::Limits` on raster reader instances.
- **Commit:** `3dd552f`

### BEAM-SEC-02: Zero-Dimension Divide-by-Zero Panics (Medium)
- **Problem:** Images or thumbnails reporting 0 width or 0 height caused integer division by zero in `generate_proxy` and float underflow in orientation transforms.
- **Fix:** Added validation returning `BeamError::Decode("Image has zero width or height")` if `width == 0 || height == 0`.
- **Commit:** `3dd552f`

### BEAM-SEC-03: Symlink Cycle and Recursion Exhaustion in Folder Scan (Medium)
- **Problem:** `LibraryDb::scan_and_import_folder` followed symlinks recursively without cycle detection or depth bounds. Circular symlinks caused infinite loops and stack exhaustion.
- **Fix:** Added `MAX_IMPORT_DEPTH = 32`, `MAX_FOLDER_IMPORT_COUNT = 100_000`, explicit `is_symlink` filtering, and canonical path tracking in `HashSet<PathBuf>` to eliminate traversal loops.
- **Commit:** `a83b97e`

### BEAM-SEC-04: Unvalidated Floating Point and Boundary Values in RON Files (Medium)
- **Problem:** Deserialized RON sidecars and presets could contain `NaN`, `+Infinity`, `-Infinity`, inverted crop rectangles (`width < 0`), or oversized history stacks, leading to invalid GPU shader uniforms or memory leaks.
- **Fix:** Added `validate_and_sanitize()` methods to `EditStack`, `Sidecar`, and `Preset`. Clamped all numerical parameters, eliminated non-finite points from tone curves, validated crop geometry bounds to `[0.0, 1.0]`, and capped history undo/redo stack depth at 200 items. Enforced `MAX_SIDECAR_FILE_SIZE = 5MB`.
- **Commit:** `b291881`

### BEAM-SEC-05: Path Traversal in Filename Export Templates (High)
- **Problem:** Template placeholders in `expand_filename_template` (e.g. `{filename}`, `{sequence}`) allowed directory traversal sequences (`..`), forward/backward slashes, control characters (`\x00` - `\x1f`), and Windows reserved device names (`CON`, `PRN`, `AUX`, `NUL`, etc.).
- **Fix:** Replaced traversal patterns, stripped control characters and directory separators, replaced Windows reserved device names with safe prefixes (`_CON`), capped stem length to 200 characters, and fell back to `"export"` if sanitized output became empty. Implemented atomic file writing (`.tmp.partial` + atomic rename) with cleanup on error.
- **Commit:** `49e10c5`

### BEAM-SEC-06: Non-Atomic File Writes (Medium)
- **Problem:** Sidecars, presets, and thumbnails were written directly to destination files. A crash, power disruption, or kill signal during write left corrupted, zero-byte, or partial files.
- **Fix:** Created `beam-core/src/fs_util.rs` with `atomic_write(path, data)`. Writes to temporary file `<name>.<pid>_<nonce>.tmp`, calls `sync_all()`, and renames atomically to target path.
- **Commit:** `b291881`

### BEAM-SEC-07: Uncaught Panics in Background Worker Threads (Medium)
- **Problem:** A panic in background decoder or export worker threads terminated the worker loop, leaving the UI thread waiting on channels indefinitely without responsive feedback.
- **Fix:** Wrapped worker task execution in `std::panic::catch_unwind(std::panic::AssertUnwindSafe(...))`. Caught panics log error diagnostics and send failure responses across channels, keeping the UI alive and responsive.
- **Commit:** `c0da755`

### BEAM-SEC-08: Unbounded In-Memory Thumbnail Cache (Medium)
- **Problem:** `LibraryViewState` stored decoded thumbnails in an unevicted `HashMap<PathBuf, TextureHandle>`. Loading large libraries containing thousands of images caused excessive RAM usage.
- **Fix:** Capped in-memory thumbnail cache at `MAX_IN_MEMORY_THUMBNAILS = 500` entries using FIFO eviction. Bounded pending background thumbnail requests to 200 items.
- **Commit:** `c0da755`

### BEAM-SEC-09: Unchecked Database Schema Evolution (Low)
- **Problem:** Older application versions reading newer SQLite database schemas could misinterpret modified tables or corrupt state.
- **Fix:** Added `CURRENT_SCHEMA_VERSION = 1` verification in `LibraryDb::new`. Databases with versions exceeding supported schema are rejected with a descriptive error.
- **Commit:** `a83b97e`

### BEAM-SEC-10: Licensing and Supply-Chain Compliance (Low)
- **Problem:** Repository lacked an explicit root `LICENSE` file for GPL-3.0-only; `Cargo.lock` was not tracked in git; `cargo-deny` configuration was missing.
- **Fix:** Added root GPL-3.0 `LICENSE`, tracked `Cargo.lock`, configured `deny.toml` with license allowances compatible with GPL-3.0, and updated repository clone URLs.
- **Commit:** `a6448de`

### BEAM-SEC-11: Integer Overflow in GPU Buffer Sizing and Zero Dimensions (Medium)
- **Problem:** `GpuImage::from_rgba32f` and `render_to_rgba8_with_colorspace` calculated staging buffer sizes using `u32` arithmetic (`bytes_per_row * height`), which could overflow on high-resolution image targets (e.g. 100+ Megapixels).
- **Fix:** Switched buffer size arithmetic to `u64`: `(bytes_per_row as u64).checked_mul(height as u64)`. Guarded against zero width or height dimensions before GPU resource allocation.
- **Commit:** `1db83ff`

### BEAM-SEC-12: Unbounded Embedded ICC Profile Allocation (Medium)
- **Problem:** Decoding images with embedded ICC color profiles read arbitrary profile byte lengths into memory without size validation.
- **Fix:** Enforced `MAX_ICC_PROFILE_SIZE = 4 * 1024 * 1024` (4 MB) limit in `beam-media/src/decoder.rs`. Profiles exceeding this threshold are safely skipped, falling back to standard sRGB.
- **Commit:** `3dd552f`

---

## 4. Automated Tooling Results

### 4.1. Cargo Audit
Executed `cargo audit --deny warnings` against 455 crate dependencies.
- Advisory Database: RustSec Advisory DB (1,395 advisories loaded)
- Result: **0 vulnerabilities found**, exit code 0.

### 4.2. Cargo Deny
Executed `cargo deny check` with GPL-3.0 compatible policy:
- Advisories: ok (0 security advisories)
- Bans: ok (0 banned crates)
- Licenses: ok (All dependencies conform to GPL-3.0 compatible licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, 0BSD, OFL-1.1, LGPL-2.1)
- Sources: ok (crates.io and approved git repositories only)
- Exit code: 0.

### 4.3. Cargo Clippy
Executed `cargo clippy --workspace --all-targets -- -D warnings`:
- Workspace: clean (0 warnings, 0 errors, exit code 0).
- Fuzz package: clean (0 warnings, 0 errors, exit code 0).

### 4.4. Cargo Test (Debug and Release with Overflow Checks)
- Debug execution (`cargo test --workspace`):
  - `beam-core`: 36 passed, 0 failed
  - `beam-gpu`: 6 passed, 0 failed
  - `beam-library`: 6 passed, 0 failed
  - `beam-media`: 26 passed, 0 failed
  - Total: **74 passed, 0 failed**, exit code 0.
- Release execution (`RUSTFLAGS="-C debug-assertions=on -C overflow-checks=on" cargo test --workspace --release`):
  - **74 passed, 0 failed**, exit code 0.

---

## 5. Property-Based Testing (proptest)

Property tests were implemented using `proptest` across critical math and parser surfaces:
1. **Tone Curve and HSL Space (`beam-core/src/color.rs`)**:
   - `prop_rgb_hsl_finite_and_bounded`: Verifies that arbitrary RGB inputs in `[0.0, 1.0]` convert to finite HSL values with hue in `[0, 360]`, saturation and lightness in `[0, 1]`, and roundtrip back within finite bounds.
   - `prop_tone_curve_arbitrary_points_finite_and_bounded`: Verifies that arbitrary collections of tone curve control points produce monotonic, finite, and strictly bounded outputs.
2. **Crop Geometry Invariants (`beam-core/src/sidecar.rs`)**:
   - `prop_crop_geometry_always_valid`: Tests arbitrary float values for `x`, `y`, `width`, `height`, `straighten`, and rotation. Confirms `validate_and_sanitize()` preserves bounds: `x + width <= 1.0001`, `y + height <= 1.0001`, `rotation % 90 == 0`, and `straighten` in `[-45, 45]`.
3. **Filename Template Safety (`beam-media/src/export.rs`)**:
   - `prop_filename_template_safety`: Verifies that arbitrary string inputs for filename templates never contain directory separators (`/`, `\`), never contain path traversal elements (`..`), never exceed 200 characters, and never produce empty filenames.

All property tests completed 100% of test cases without finding counterexamples.

---

## 6. Coverage-Guided Fuzzing (cargo-fuzz / libFuzzer / ASan)

Six fuzz targets were constructed under `fuzz/fuzz_targets/` to test parsers and deserializers against untrusted, malicious inputs under LLVM AddressSanitizer (`-Zsanitizer=address`):

| Fuzz Target | Target Surface | Executions | Exec/sec | Memory Errors / Crashes |
|---|---|---|---|---|
| `fuzz_sidecar_loader` | RON Sidecar Deserializer | 456,200 | 76,033 | 0 |
| `fuzz_preset_loader` | RON Preset Deserializer | 511,557 | 85,259 | 0 |
| `fuzz_config_loader` | RON App Configuration Deserializer | 539,984 | 89,997 | 0 |
| `fuzz_icc_parse` | MoxCMS ICC ColorProfile Parser | 1,801,826 | 300,304 | 0 |
| `fuzz_exif_parse` | Kamadak-Exif TIFF/EXIF Reader | 464,823 | 77,470 | 0 |
| `fuzz_image_decode` | Bounded Image Crate Reader | 247,424 | 41,237 | 0 |
| **Total** | | **4,021,814** | | **0** |

All fuzz targets completed without triggering out-of-bounds accesses, use-after-free, memory leaks, panics, or hangs.

---

## 7. Static Code Analysis and Memory Safety Summary

- **`unsafe` Blocks:** 0 instances in `crates/` or `fuzz/`. The entire workspace is 100% safe Rust.
- **`transmute` Usage:** 0 instances.
- **Raw Pointer Dereferences:** 0 instances.
- **Unchecked Unwraps in Production Paths:** All `unwrap()` occurrences in production code are guarded by defensive `is_some()` checks or are Mutex lock acquisitions.
- **Worker Thread Panics:** Worker threads are isolated by `catch_unwind` boundaries to safeguard application uptime.
- **File System Operations:** All state writes (sidecars, presets, exported media, and thumbnails) use atomic write-then-rename patterns.

---

## 8. Conclusion

The **beaM.** codebase satisfies high security and robustness standards. All identified vulnerabilities have been eliminated and backed by automated unit tests, release overflow verification, property-based tests, and coverage-guided fuzz testing. The application runs entirely within safe Rust boundaries with zero known memory safety or denial-of-service vulnerabilities.
