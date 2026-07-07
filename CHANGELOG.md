# Changelog

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.1.0] - 2026-07-07

### Added
- PHP-FPM FastCGI proxy support, configured per-server via `php:` in
  `config.yaml` (`enabled`, `fpm_socket`, `extension`, `index`).
- `fpm_socket` supports both Unix sockets (`unix:/path/to.sock`) and TCP
  (`tcp:host:port`).
- Requests matching the configured PHP extension (or a directory falling
  back to the configured index file) are forwarded to php-fpm; everything
  else continues to be served as static files, unchanged.
- Path-traversal guard on resolved PHP script paths.

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
