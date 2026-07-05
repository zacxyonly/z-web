# Changelog

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
