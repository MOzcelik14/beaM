# beaM.

**beaM.** (pronounced *beam dot*) is an open-source, non-destructive RAW photo editor and library manager for Linux and Windows — designed as a modern, high-performance Lightroom alternative.

Licensed under the **GNU General Public License v3.0 (GPL-3.0)**.

---

## Key Features & Core Principles

1. **Non-destructive Editing**: The original RAW file is never altered. Edits are recorded in human-readable RON sidecar files (`<filename>.beam.ron`) with schema versioning for future backward-compatible migrations.
2. **GPU Pipeline**: Modular WGSL shaders executing discrete adjustment stages over linear floating-point textures.
3. **Color Correctness**: Operates internally in a wide-gamut, linear working space (**linear Rec.2020**), transforming to sRGB for SDR display presentation and export.
4. **Pure Rust Decoding**: Decodes camera RAW formats natively without LibRaw or C++ dependencies using the `rawler` crate.
5. **Responsive Off-Thread Architecture**: Decoding and heavy operations execute asynchronously on background threads; the egui UI never blocks.

---

## Repository Architecture

```
beam/
├── Cargo.toml                # Workspace manifest with shared dependencies
├── crates/
│   ├── beam-app/             # Binary application: egui UI, viewport, state management
│   ├── beam-core/            # Display name constant (beaM.), edit model, sidecars, color science, errors
│   ├── beam-gpu/             # wgpu pipeline, texture management, shader passes
│   ├── beam-media/           # Camera RAW decoding via rawler, demosaicing, raster fallbacks
│   └── beam-library/         # SQLite library catalog, import, ratings, filters
├── shaders/                  # WGSL shaders
│   └── display_transform.wgsl# Linear Rec.2020 to sRGB display transfer pass
└── README.md
```

---

## Build and Run Instructions

### Prerequisites
- **Rust**: Stable toolchain (1.89+ recommended, tested on Rust 1.99).
- **Vulkan / GPU Drivers**:
  - **Linux**: Mesa Vulkan drivers (`mesa-vulkan-drivers`) or proprietary Nvidia drivers.
  - **Windows**: Modern GPU drivers supporting Vulkan or DirectX 12.

### Building
Clone the repository and compile the workspace:
```bash
git clone https://github.com/mozcelik/beaM.git
cd beaM
cargo build --release
```

### Running the App
Run the native application:
```bash
cargo run --release -p beam-app
```

Or pass an image directly via command line:
```bash
cargo run --release -p beam-app -- /path/to/sample.ARW
```

### Running Tests and Linting
```bash
# Run all unit and integration tests across the workspace
cargo test --workspace

# Check formatting
cargo fmt --check

# Check linter warnings
cargo clippy --workspace --all-targets -- -D warnings
```

---

## Roadmap

- [x] **Milestone 1 – Skeleton**:
  - Cargo workspace with 5 modular crates.
  - Display name `APP_NAME = "beaM."` centralized in `beam-core`.
  - Pure-Rust RAW decoding with `rawler` and bilinear demosaicing.
  - Linear wide-gamut (Rec.2020) color transformation and gamma transfer function math.
  - `wgpu` rendering pipeline executing WGSL shader (`shaders/display_transform.wgsl`) to render directly in `egui`.
  - Non-blocking async background decoding channel.
  - Sidecar format (`.beam.ron`) serialization with migration test.
  - SQLite database initialization (`beam-library`).
- [x] **Milestone 2 – Edit pipeline**:
  - Develop view with right-side adjustment panel.
  - GPU shader stages: exposure (`exposure.wgsl`), white balance (`white_balance.wgsl`), contrast (`contrast.wgsl`).
  - Ping-pong intermediate rendering pipeline with reorderable and skippable passes.
  - Live preview on proxy image (~1920px max dimension) for sub-millisecond slider interaction.
  - Reset-per-slider `↺` and Reset All adjustments buttons.
  - Sidecar persistence: automatically loads edits on file open and saves on edit change.
  - Golden-image test with GPU readback verifying mathematical exposure doubling.
- [x] **Milestone 3 – More adjustments**:
  - Highlights/shadows tone mapping.
  - Draggable tone curve widget.
  - 8-band HSL adjustments (hue, saturation, luminance).
  - Fast histogram widget.
- [x] **Milestone 4 – Library**:
  - Folder import and SQLite photo cataloging.
  - Thumbnail grid with on-disk thumbnail cache.
  - Star ratings, color labels, and search filters.
- [x] **Milestone 5 – Export**:
  - Full-resolution rendering pipeline export to JPEG and PNG.
  - Selectable dimensions, quality, and color spaces.
  - Batch export with progress tracking.
- [x] **Milestone 6 – Fix: Library thumbnails display & background generator**:
  - Root cause diagnosis: resolved background queue deadlocks and missing UI repaint requests.
  - Lazy thumbnail texture loading with LRU cache in `egui`.
  - Non-blocking background worker thread for thumbnail decoding and generation.
- [x] **Milestone 7 – Support standard image formats (JPEG, PNG, TIFF, WebP)**:
  - Native pure-Rust decoding for JPEG, PNG, TIFF (8-bit & 16-bit), and WebP alongside Camera RAW.
  - Embedded ICC profile extraction and color transformation via `moxcms` into linear Rec.2020.
  - 16-bit channel depth preservation without quantization to 8-bit.
  - EXIF metadata extraction (orientation, ISO, shutter speed, aperture, camera make/model).
  - Library filtering by format (All, RAW, JPEG, PNG, TIFF, WebP).

- [x] **Milestone 8 – Localization (English + Turkish)**:
  - High-performance, thread-safe localization layer using Mozilla's Project Fluent (`fluent-bundle`).
  - Full translation catalogs for English (`locales/en.ftl`) and Turkish (`locales/tr.ftl`) covering 100% of UI elements (menus, status bar, library, develop sliders, HSL, curves, export, settings, about dialog).
  - Accurate photography terminology in Turkish (*Pozlama, Kontrast, Beyaz Dengesi, Sıcaklık, Renk Tonu, Doygunluk, Parlaklık, Işıklar, Gölgeler, Ton Eğrisi, Histogram, Dışa Aktar, Kütüphane, Geliştir, Ayarlar*).
  - Automatic system language detection on startup with user preference stored in `~/.config/beam/config.ron` (Linux) or `%APPDATA%\beam\config.ron` (Windows).
  - Settings dialog (`Ctrl+,`) with instant live language switching and reactive repaint.
  - Automated compile-time key parity unit testing.

- [x] **Milestone 9 – Complete the export options**:
  - Color space conversion on export: sRGB, Display P3, and Adobe RGB (1998) via GPU shader stage and linear algebra transformation matrices.
  - Standard embedded ICC profiles generated pure-Rust via `moxcms` (`new_srgb()`, `new_display_p3()`, `new_adobe_rgb()`) and tagged directly into JPEG/PNG metadata.
  - Image resizing options: original dimensions, long-edge scaling, percentage scaling, and exact dimensions with aspect ratio lock, using high-quality Lanczos3 filtering.
  - Output sharpening: configurable unsharp mask for Screen or Print with separable Gaussian blur.
  - EXIF metadata preservation: transfers camera capture metadata while normalizing the orientation tag to upright (1).
  - Flexible file naming template engine supporting variables (`{name}`, `{date}`, `{counter}`, `{rating}`) with dynamic preview.
  - Collision-avoidance logic preventing accidental source or existing file overwrite.
  - Zero-seam tiled GPU processing: automatically divides images exceeding 2048px into discrete tiles, strictly bounding GPU memory (< 70 MB) even for 40+ MP and 100 MP images.
  - Full bilingual localization in English and Turkish.

- [x] **Milestone 10 – Undo/redo, history, before/after**:
  - Linear undo/redo stack (`Ctrl+Z`, `Ctrl+Shift+Z` / `Ctrl+Y`, Edit menu) supporting up to 50 operations.
  - Intelligent grouping semantics: continuous slider drags and curve point movements group into a single commit on pointer release, while discrete actions (resets, checkboxes, Reset All) commit immediately.
  - Collapsible History panel on the left side of the Develop view with jump-to-any-step on click, past/future visual indicators, and persistent step numbering.
  - Sidecar persistence: history stack and active history index are serialized to and restored from `.beam.ron`.
  - Zero-latency Before/After comparison modes powered by cached original unedited proxy texture:
    - **Toggle Mode (`\`)**: Rapidly switch between edited image and original with a subtle `BEFORE` badge overlay.
    - **Split View Mode**: Interactive split screen with draggable divider bar (`↔` handle) and zero-seam UV slicing in the viewport painter.
    - **Side-by-Side Mode**: Dual-viewport comparison displaying before and after versions simultaneously with aligned aspect ratios and badges.
  - Complete English and Turkish localization for all history entries, comparison toolbar labels, and tooltips.

- [x] **Milestone 11 – Crop, rotate, straighten**:
  - Interactive crop overlay widget in viewport with draggable L-bracket corner and edge handles, rule-of-thirds compositional grid, dimmed exterior mask, and live cursor feedback.
  - Aspect ratio constraint presets: Original, 1:1 Square, 4:5 (8:10), 3:2, 16:9, and Free/Custom, with automatic centered crop box positioning.
  - Lossless 90° clockwise and counter-clockwise rotation and horizontal/vertical flipping.
  - High-precision continuous straighten slider (-45.0° to +45.0°) with 1-click reset.
  - Hardware-accelerated GPU display shader pipeline with universal 4-tap manual bilinear texture sampling (`sample_input_bilinear`) ensuring crisp, artifact-free subpixel rendering on all hardware.
  - Dual display modes: shows uncropped photo under crop overlay during active editing mode (`C` shortcut / `Enter` or `Esc` to finish), and seamlessly clips to cropped aspect ratio during normal viewing.
  - Full export pipeline and thumbnail generation integration: applies orientation, flips, straighten, and crop box before resizing and sharpening.
  - Complete English and Turkish localization across all UI elements, presets, and history records.

---

## Adding a New Language

beaM. uses Mozilla's **Project Fluent** format (`.ftl`) for flexible, natural localization.

To contribute a new language (e.g. German, French, Spanish):

1. **Create the Catalog File**:
   Copy `locales/en.ftl` to `locales/<lang_code>.ftl` (e.g., `locales/de.ftl`):
   ```bash
   cp locales/en.ftl locales/de.ftl
   ```
   Translate each message while keeping the identifiers and arguments (such as `{$count}` or `{$band}`) unchanged.

2. **Register the Language in `beam-core`**:
   Open `crates/beam-core/src/i18n.rs`:
   - Add the FTL file inclusion:
     ```rust
     const DE_FTL: &str = include_str!("../../../locales/de.ftl");
     ```
   - Add the variant to the `Language` enum:
     ```rust
     pub enum Language {
         En,
         Tr,
         De, // New language
     }
     ```
   - Implement `code()` (e.g. `"de"`), `display_name()` (e.g. `"Deutsch"`), and update `from_locale_str()`.
   - Add the bundle instantiation in `I18n::new()`.

3. **Verify Catalog Parity**:
   Run the parity test to ensure all translation keys match:
   ```bash
   cargo test -p beam-core test_translations_parity
   ```

4. **Register in the Settings UI**:
   In `crates/beam-app/src/app.rs`, add the new `Language` variant to the language selection loop in `show_settings_dialog_window()`.

---

## Design Decisions

1. **Working Color Space**: We selected **Linear Rec.2020** as the primary working space. Rec.2020 encompasses virtually all real-world surface colors and modern camera sensor gamuts while maintaining physically accurate linearity for exposure, white balance, and contrast mathematics.
2. **Ping-Pong Modular Pipeline**: Adjustments are structured as independent WGSL shader passes executing between two ping-pong intermediate textures (`Rgba32Float`). Each stage receives its own dedicated uniform buffer, ensuring passes can be reordered, bypassed, or extended without rebuilding pipeline resources.
3. **Linear Contrast & White Balance**:
   - Contrast operates in linear light pivoted around middle-gray ($M = 0.18$) using an exponential slope: $x' = M \cdot (x / M)^{2^{C / 100}}$. This preserves midtone tonality without harsh clipping.
   - White balance computes chromatic adaptation gains $(G_r, G_g, G_b)$ from Kelvin temperature and Green-Magenta tint offsets, applied directly to linear channel amplitudes.
4. **Proxy for Interactive Responsiveness**: Decoded RAW files generate a fast bilinear proxy (~1920px max dimension). Sliders manipulate the proxy on the GPU in <1ms without re-uploading texture data, guaranteeing 60+ FPS responsiveness.
5. **WGSL Shader Sampling & Texel Loading**: For HDR 32-bit floating point textures (`Rgba32Float`), standard hardware samplers often require vendor-specific filterable extensions. We utilize unfilterable texture bindings with direct texel loading (`textureLoad`) in the display pass, avoiding any GPU driver compatibility issues across Intel, Nvidia, AMD, and software rasterizers.
6. **First-Class Standard Image Formats & Color Management**: Standard raster formats (JPEG, PNG, TIFF 8/16-bit, WebP) are fully supported first-class formats alongside Camera RAW. Unlike RAW files, they are display-referred: embedded ICC profiles are parsed via `moxcms` and transformed into linear Rec.2020 (falling back to sRGB transfer when absent). 16-bit TIFF/PNG channels are preserved directly into `f32` normalized values without intermediate 8-bit quantization. EXIF tags and image orientation (1..=8) are parsed natively via `kamadak-exif` and applied losslessly.
7. **Sidecar Format**: We use human-readable [RON](https://github.com/ron-rs/ron) format with an explicit `format_version: u32` header, allowing human inspectability, git diffability, and version migration.
8. **Internationalization & Typography**: We chose Mozilla's **Project Fluent** (`fluent-bundle`) over gettext. Fluent natively supports grammatical gender, plural rules, and natural bidirectional strings. To avoid UI spacing anomalies with desktop GUI toolkits, bundles are initialized with Unicode isolating marks disabled (`bundle.set_use_isolating(false)`). All Turkish Latin Extended-A characters (`ç`, `Ç`, `ğ`, `Ğ`, `ı`, `I`, `i`, `İ`, `ö`, `Ö`, `ş`, `Ş`, `ü`, `Ü`) are natively supported by egui's default typography without requiring auxiliary font packs.


