# Crafting Table

![Rust](https://img.shields.io/badge/language-Rust-orange?style=for-the-badge&logo=rust)
![License](https://img.shields.io/badge/license-MIT-blue?style=for-the-badge)
![Platform](https://img.shields.io/badge/platform-linux-lightgrey?style=for-the-badge)

> A Rust terminal app for **media conversion**, **image conversion**, and **downloads**.

---

## 📦 Table of Contents

- [Features](#-features)
- [Requirements](#-requirements)
- [Installation](#-installation)
- [Usage](#-usage)
- [Supported Image Formats](#-supported-image-formats)
- [Settings](#-settings)
- [External Dependencies](#-external-dependencies)
- [License](#-license)

---

## 🚀 Features

| Category | Capability |
|----------|------------|
| **Image conversion** | Native Rust conversion between AVIF, GIF, JPEG, PNG, and WebP |
| **Direct downloads** | HTTP(S) downloads with byte ranges and up to 16 concurrent connections |
| **Speed limiting** | Single-connection downloads when a speed limit is configured |
| **Video/audio** | Conversion and scaling via FFmpeg |
| **Video-site extraction** | Broad extraction via yt-dlp |
| **Gallery-site extraction** | General extraction via gallery-dl |
| **Settings** | API key management, browser selection for cookies, searchable public API catalog |

---

## ⚙️ Requirements

### Rust toolchain
Install [Rust](https://rustup.rs/) to build and run the project:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### External programs (optional, feature-dependent)

| Feature | Required program | Notes |
|---------|------------------|-------|
| Video/audio conversion | [FFmpeg](https://ffmpeg.org/) | Codec support |
| Video-site extraction | [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Site-specific extraction |
| Gallery-site extraction | [gallery-dl](https://github.com/mikf/gallery-dl) | Site-specific extraction |

The Rust-implemented features (image conversion, direct downloads) do **not** require FFmpeg, yt-dlp, gallery-dl, aria2c, ImageMagick, or Python.

---

## 🛠️ Installation

Clone the repository:

```sh
git clone https://github.com/YOUR_USERNAME/craftingtable.git
cd craftingtable
```

### Run directly

```sh
cargo run
```

### Build a release executable

```sh
cargo build --release
```

The binary will be at `target/release/craftingtable`.

---

## 🖼️ Supported Image Formats

Native image conversion (decoded as a still image for animated inputs):

- **AVIF**
- **GIF**
- **JPEG**
- **PNG**
- **WebP**

Formats are enabled via the `image` crate features in `Cargo.toml`.

---

## ⚙️ Settings

The Settings menu lets you:

- Manage source-specific API keys
- Choose where the key store is kept (the existing store is moved when the destination does not already contain a file)
- Set the default browser for downloader cookies (used by yt-dlp and gallery-dl; overridable per yt-dlp download)

Saved settings are:
- Scoped to a host
- Passed to matching gallery downloads through a temporary config file
- Stored locally with owner-only permissions on Unix
- Kept in the user's configuration directory

### Public API catalog

Settings includes a searchable, category-based catalog sourced from the [public-apis repository](https://github.com/public-apis/public-apis). Selecting an entry opens its documentation in the system browser.

> An internet connection is required to fetch the latest catalog. The catalog provides API listings and authentication requirements — **not** API keys. Obtain credentials from each API provider.

---

## 📄 License

This project is licensed under the **MIT License**. See the [LICENSE](LICENSE) file for details.

---

<p align="center">
  <b>Crafting Table</b> — media conversion, image conversion, and downloads in your terminal.
</p>
