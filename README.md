<div align="center">

# ⚡ z-web

**Lightweight, multi-server static file server with automatic hot reload and optional PHP execution.**

[![CI](https://github.com/zacxyonly/z-web/actions/workflows/ci.yml/badge.svg)](https://github.com/zacxyonly/z-web/actions/workflows/ci.yml)
[![Release](https://github.com/zacxyonly/z-web/actions/workflows/release.yml/badge.svg)](https://github.com/zacxyonly/z-web/actions/workflows/release.yml)
[![GitHub Release](https://img.shields.io/github/v/release/zacxyonly/z-web?logo=github&color=blue)](https://github.com/zacxyonly/z-web/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange?logo=rust)](https://www.rust-lang.org)

[Quick Start](#-quick-start) •
[Installation](#-installation) •
[Configuration](#️-configuration) •
[PHP Support](#-php-support) •
[Endpoints](#-http-endpoints) •
[Releasing](#-releases--cicd)

</div>

---

## 🌟 Highlights

- **⚡ Blazing Fast Asynchronous Core** — Built on [Axum](https://github.com/tokio-rs/axum) and [Tokio](https://tokio.rs/) for minimal memory footprint and high throughput.
- **🔄 Instant Live Reload** — Real-time browser auto-refresh via WebSockets (`/livereload`) on file changes with smart event debouncing.
- **🌐 Multi-Server in One Binary** — Host multiple websites or ports concurrently from a single configuration file and process.
- **⚛️ SPA Mode & Custom 404** — Native fallback to `index.html` for client-side routing (React, Vue, Vite) and automatic `404.html` delivery.
- **⚙️ Live Configuration Reloading** — Add, remove, or modify server entries in `config.yaml` on the fly without stopping running instances.
- **🐘 Plug-and-Play PHP Execution** — Optional `.php` execution via `php-cgi` without running separate daemon pools like PHP-FPM.
- **🛡️ Built-in Security Hardening** — Sandboxed directory traversal prevention, HTTPOXY (CVE-2016-5385) mitigation, and 30s script execution timeouts.
- **📦 True Static Binaries** — Fully self-contained musl binaries for Linux, universal macOS binaries, and native Windows builds.
- **🗜️ Automatic Gzip Compression & CORS** — Built-in compression for static text assets and permissive CORS for cross-port local API testing.

---

## 🚀 Quick Start

Get a web server running in 5 seconds with zero setup:

```bash
# 1. Download or compile z-web
./z-web
```

On its very first launch, `z-web` will automatically:
1. Create a clean default `config.yaml` listening on `0.0.0.0:8080`.
2. Generate a `web/` folder containing an interactive starter page with hot reload enabled.
3. Open `http://localhost:8080` in your browser!

```text
2026-10-18T19:30:00Z  INFO z-web v1.0.0 starting
2026-10-18T19:30:00Z  INFO Server listening addr=0.0.0.0:8080 folder="web"
2026-10-18T19:30:00Z  INFO All servers running. Edit config.yaml to add/remove servers. Ctrl+C to stop.
```

---

## 📦 Installation

### Option 1: Pre-compiled Binaries (Recommended)

Download the pre-built binary for your platform from the [GitHub Releases](https://github.com/zacxyonly/z-web/releases) page:

| Platform | Architecture | Archive | Type |
| :--- | :--- | :--- | :--- |
| **Linux** | `x86_64` | `z-web-*-x86_64-unknown-linux-musl.tar.gz` | Fully Static (Alpine, Ubuntu, Debian, CentOS) |
| **Linux** | `aarch64` | `z-web-*-aarch64-unknown-linux-gnu.tar.gz` | ARM64 / Raspberry Pi / AWS Graviton |
| **macOS** | Apple Silicon (`M1/M2/M3/M4`) | `z-web-*-aarch64-apple-darwin.tar.gz` | Native ARM64 |
| **macOS** | Intel (`x86_64`) | `z-web-*-x86_64-apple-darwin.tar.gz` | Intel 64-bit |
| **Windows** | `x86_64` | `z-web-*-x86_64-pc-windows-msvc.zip` | Windows 64-bit Executable |

**Linux & macOS Quick Install:**
```bash
# Extract and run
tar -xzf z-web-*-x86_64-unknown-linux-musl.tar.gz
chmod +x z-web
./z-web
```

### Option 2: Build from Source

Requires the [Rust toolchain](https://rustup.rs/) (edition 2021+):

```bash
git clone https://github.com/zacxyonly/z-web.git
cd z-web
cargo build --release
```
The optimized binary will be located at `target/release/z-web`.

---

## 💻 CLI Usage

```text
Usage: z-web [OPTIONS]

Options:
  -c, --config <FILE>    Custom path to config.yaml (default: config.yaml)
  -v, --version          Print version information
  -h, --help             Print help information
```

### Logging Configuration
Control log output verbosity using the standard `RUST_LOG` environment variable:
```bash
RUST_LOG=debug ./z-web       # Detailed debug logs (watcher events, ws frames)
RUST_LOG=z_web=info ./z-web  # Standard informative logs (default)
RUST_LOG=z_web=warn ./z-web  # Warnings and errors only
```

---

## ⚙️ Configuration

The default configuration file is **`config.yaml`** (or specify a custom path with `-c /path/to/config.yaml`):

```yaml
# z-web configuration
servers:
  # Public frontend site (with SPA routing enabled)
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"
    spa: true                # fallback 404s to index.html for client-side routing

  # Internal admin dashboard with PHP support
  - ip: "127.0.0.1"
    port: 8081
    web_folder: "web_admin"
    php: true
    # php_cgi_path: "/usr/bin/php-cgi" # optional custom binary path
```

### Configuration Reference

| Option | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `ip` | `string` | *(Required)* | The IP address interface to bind to (e.g. `0.0.0.0` or `127.0.0.1`). |
| `port` | `integer` | *(Required)* | The network port (1–65535). Duplicate ports are safely validated. |
| `web_folder` | `string` | *(Required)* | Path to the directory of static/PHP assets to serve. |
| `spa` | `boolean` | `false` | Enable Single Page Application mode: missing routes fallback to `index.html` with status 200. |
| `php` | `boolean` | `false` | Enable execution of `.php` scripts via `php-cgi`. |
| `php_cgi_path`| `string` | `"php-cgi"` | Path or executable name for PHP-CGI (searched on `$PATH` by default). |

### Custom 404 Pages
When `spa: false`, if a file named `404.html` exists in `web_folder`, `z-web` will automatically serve it with HTTP status `404 Not Found` whenever a static file is missing.

### 🔥 Hot Reloading Config
You can edit `config.yaml` at any time while `z-web` is running:
- **Adding a server**: Automatically binds and starts the new server instance immediately.
- **Removing a server**: Gracefully terminates only the removed server instance.
- **Changing folder / PHP**: Automatically restarts that specific instance with the new settings without dropping other servers.

---

## 🐘 PHP Support

When `php: true` is configured for a server, requests targeting `.php` files (or directories containing an `index.php` fallback) are dynamically executed via `php-cgi` conforming to the CGI/1.1 specification.

### Installing PHP-CGI

Ensure `php-cgi` is installed on your host system:

```bash
# Debian / Ubuntu
sudo apt install php-cgi

# Alpine Linux
apk add php83-cgi

# macOS (Homebrew)
brew install php

# Arch Linux
sudo pacman -S php-cgi
```

### Security & Architecture
- **Isolated Execution**: Runs as classic CGI per-request. No persistent background FastCGI socket or complex service configuration required.
- **Path Traversal Guard**: Script paths are canonicalized and verified to strictly reside within `web_folder`. Attempts to escape (`..` or directory symlinks) are rejected with `404 Not Found`.
- **HTTPOXY Protection**: Automatically strips incoming `Proxy` request headers to defend against CVE-2016-5385.
- **Execution Timeout**: Protects against hung scripts, blocking deadlocks, or runaway loops with an automatic 30-second cancellation threshold.

---

## 📡 HTTP Endpoints

Every server instance provides the following routes:

| Route | Method | Description |
| :--- | :--- | :--- |
| `GET /healthz` | `GET` | Health check endpoint returning JSON `{ "status": "ok", "version": "1.0.0" }`. |
| `GET /livereload` | `GET` | WebSocket connection stream that emits `"reload"` events on file system changes. |
| `GET /*` | `GET`, `HEAD` | Static file delivery with MIME detection, index resolution, and gzip compression. |

---

## 🔄 Live Reload Integration

In your HTML pages, connect to `/livereload` using standard WebSockets:

```html
<script>
  (function() {
    var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    var ws = new WebSocket(proto + '//' + location.host + '/livereload');
    ws.onmessage = function(e) { if (e.data === 'reload') location.reload(); };
    ws.onclose = function() { setTimeout(function() { location.reload(); }, 2000); };
  })();
</script>
```
*(Note: Starter pages generated in new web folders include this snippet automatically).*

---

## 🛠️ Development & Testing

```bash
# Run unit tests
cargo test

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Check code formatting
cargo fmt --all -- --check
```

---

## 🚀 Releases & CI/CD

Releases are automated via GitHub Actions using the manual dispatch workflow ([`.github/workflows/release.yml`](.github/workflows/release.yml)):

1. Go to the **Actions** tab in GitHub.
2. Select **Release** from the workflow list.
3. Click **Run workflow**, enter the version number (e.g. `1.0.0`), and submit.
4. The workflow will run full test suites, tag the release, compile cross-platform binaries, calculate SHA-256 checksums, and publish the release.

---

## 📄 License

This project is licensed under the terms of the [MIT License](LICENSE).

## ☕ Support

If `z-web` is useful to your workflow, you can support ongoing development at [trakteer.id/zacxyonly](https://trakteer.id/zacxyonly).
