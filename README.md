<p align="center">
  <img src="assets/logo.png" alt="beaM." width="160" />
</p>

<h1 align="center">beaM.</h1>

<p align="center">
  Non-destructive RAW photo editor and local library manager for Linux and Windows.
</p>

<p align="center">
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/Version-0.8.0-blue.svg" alt="Version 0.8.0" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-green.svg" alt="License: GPL-3.0" /></a>
  <img src="https://img.shields.io/badge/Platform-Linux%20%7C%20Windows-lightgrey.svg" alt="Platform: Linux | Windows" />
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.tr.md">Türkçe</a>
</p>

---

## Overview

**beaM.** is a fast, privacy-first desktop photo studio built for photographers and visual artists. Your original camera files remain untouched on disk while all adjustments are saved in lightweight sidecar files—no cloud subscriptions or accounts required.

## Key Features

- **Non-Destructive Workflow**: Original RAW and raster images are never modified; edits are stored cleanly in `.beam` sidecar files with a 50-step undo/redo history and reusable presets.
- **Color & Tone Control**: Exposure, Contrast, Highlights, Shadows, White Balance (Temperature & Tint), interactive Tone Curve, and 8-band HSL color adjustments.
- **Geometry & Framing**: Freeform and aspect-ratio crops (1:1, 4:5, 3:2, 16:9), 90° rotation, flips, and continuous horizon straightening.
- **Comparison & Inspection**: Before/after toggle, draggable split view, side-by-side comparison, and live RGB/luminance histogram with clipping indicators.
- **Local Photo Library**: Fast folder browser, bookmarks, star ratings, color labels, EXIF metadata inspection, and filmstrip navigation.
- **High-Resolution Batch Export**: Export to JPEG or 8/16-bit PNG with sRGB, Display P3, or Adobe RGB color profiles, output sharpening, and dynamic filename templates.

## Supported Formats

| Category | Extensions |
| :--- | :--- |
| **Camera RAW** | `.arw`, `.cr2`, `.cr3`, `.nef`, `.raf`, `.rw2`, `.dng`, `.pef`, `.orf` |
| **Standard Images** | `.jpg`, `.jpeg`, `.png`, `.tif`, `.tiff`, `.webp` |

---

## Download & Installation

You can install and update **beaM.** automatically via [prisM. Suite](https://github.com/MOzcelik14/prism-suite), or download packages from [GitHub Releases](https://github.com/MOzcelik14/beaM/releases):

- **Debian / Ubuntu (`.deb`)**: `sudo dpkg -i beam_0.8.0_amd64.deb && sudo apt-get install -f`
- **Fedora / RHEL (`.rpm`)**: `sudo rpm -i beam-0.8.0-1.x86_64.rpm`
- **Linux Portable (`.AppImage`)**: `chmod +x beaM-0.8.0-x86_64.AppImage && ./beaM-0.8.0-x86_64.AppImage`
- **Windows**: Run `beaM-v0.8.0-windows-x86_64-setup.exe` or extract `beaM-v0.8.0-windows-x86_64.zip`.

## System Requirements

- **OS**: Linux (glibc 2.31+, x86_64) or Windows 10/11 (64-bit)
- **Graphics**: GPU with Vulkan 1.2+ (Linux/Windows) or DirectX 12 (Windows)
- **Memory**: 4 GB RAM minimum (8 GB recommended for batch exports)

## Building from Source

Requires Rust 1.89+:

```bash
git clone https://github.com/MOzcelik14/beaM.git
cd beaM
cargo build --release -p beam-app
```

## License

Licensed under the **GNU General Public License v3.0 (GPL-3.0)**. See [LICENSE](LICENSE) for details.
