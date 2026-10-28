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
- [ ] **Milestone 3 – More adjustments**:
  - Highlights/shadows tone mapping.
  - Draggable tone curve widget.
  - 8-band HSL adjustments (hue, saturation, luminance).
  - Fast histogram widget.
- [ ] **Milestone 4 – Library**:
  - Folder import and SQLite photo cataloging.
  - Thumbnail grid with on-disk thumbnail cache.
  - Star ratings, color labels, and search filters.
- [ ] **Milestone 5 – Export**:
  - Full-resolution rendering pipeline export to JPEG and PNG.
  - Selectable dimensions, quality, and color spaces.
  - Batch export with progress tracking.

---

## Design Decisions

1. **Working Color Space**: We selected **Linear Rec.2020** as the primary working space. Rec.2020 encompasses virtually all real-world surface colors and modern camera sensor gamuts while maintaining physically accurate linearity for exposure, white balance, and contrast mathematics.
2. **Ping-Pong Modular Pipeline**: Adjustments are structured as independent WGSL shader passes executing between two ping-pong intermediate textures (`Rgba32Float`). Each stage receives its own dedicated uniform buffer, ensuring passes can be reordered, bypassed, or extended without rebuilding pipeline resources.
3. **Linear Contrast & White Balance**:
   - Contrast operates in linear light pivoted around middle-gray ($M = 0.18$) using an exponential slope: $x' = M \cdot (x / M)^{2^{C / 100}}$. This preserves midtone tonality without harsh clipping.
   - White balance computes chromatic adaptation gains $(G_r, G_g, G_b)$ from Kelvin temperature and Green-Magenta tint offsets, applied directly to linear channel amplitudes.
4. **Proxy for Interactive Responsiveness**: Decoded RAW files generate a fast bilinear proxy (~1920px max dimension). Sliders manipulate the proxy on the GPU in <1ms without re-uploading texture data, guaranteeing 60+ FPS responsiveness.
5. **WGSL Shader Sampling & Texel Loading**: For HDR 32-bit floating point textures (`Rgba32Float`), standard hardware samplers often require vendor-specific filterable extensions. We utilize unfilterable texture bindings with direct texel loading (`textureLoad`) in the display pass, avoiding any GPU driver compatibility issues across Intel, Nvidia, AMD, and software rasterizers.
6. **Graceful Raster Fallback**: While camera RAW formats (ARW, CR2, CR3, NEF, DNG, RAF, etc.) are prioritized through `rawler`, standard raster formats (PNG, JPEG) are supported via a fallback to ensure testing and mixed-library viewing. Corrupted or invalid files surface clear error banners in the UI rather than crashing.
7. **Sidecar Format**: We use human-readable [RON](https://github.com/ron-rs/ron) format with an explicit `format_version: u32` header, allowing human inspectability, git diffability, and version migration.
