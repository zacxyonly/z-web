# Changelog

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.1.0] - 2026-07-05

### Added
- Optional PHP support: set `php: true` on a server entry to execute `.php`
  files (and `index.php` for directory requests) via `php-cgi`, instead of
  serving them as static text.
  - New per-server config fields: `php` (bool, default `false`) and
    `php_cgi_path` (string, default `"php-cgi"`).
  - Uses classic CGI (one `php-cgi` process per request) — no extra
    long-running service, no new dependencies beyond PHP itself on the host.
  - Path resolution is sandboxed to `web_folder`; `..` traversal and symlink
    escapes are rejected.
  - Fully backward compatible: existing `config.yaml` files without a `php`
    field keep working unchanged, with PHP execution off by default.

## [1.0.0] - 2026-07-05

First public release.

### Added
- Multi-server static file serving from a single `config.yaml`.
- Config hot reload — add/remove servers without restarting the process.
- File hot reload via WebSocket (`/livereload`) — browser auto-refreshes on change.
- Gzip compression for text assets.
- `/healthz` endpoint for uptime monitoring.
- Graceful shutdown on Ctrl+C / SIGTERM.
- Config validation (duplicate port detection) before servers are spawned.
- Unit tests for config parsing and validation.
- CI workflow (build, test, clippy, fmt check) via GitHub Actions.

### Changed
- `ServerConfig::socket_addr()` and `ServerConfig::ensure_folder()` now return
  `Result` instead of panicking — a single misconfigured server no longer
  takes down the whole process.
