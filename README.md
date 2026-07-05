# z-web

![CI](https://github.com/zacxyonly/z-web/actions/workflows/ci.yml/badge.svg)
![license](https://img.shields.io/badge/license-MIT-blue.svg)
![rust](https://img.shields.io/badge/rust-2021-orange.svg)

Lightweight multi-server static file server with hot reload. Zero config to get started.

## Installation

Requires the [Rust toolchain](https://rustup.rs/) (stable, edition 2021+).

```bash
git clone https://github.com/zacxyonly/z-web.git
cd z-web
cargo build --release
```

The compiled binary is at `target/release/z-web`.

## Quick Start

```bash
./target/release/z-web
```

On first run, `config.yaml` is auto-created with a single server on port 8080.

## Config

Single source of truth: **`config.yaml`** — no other formats supported.

```yaml
servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"

  - ip: "127.0.0.1"
    port: 8081
    web_folder: "web_admin"
```

**Hot reload:** edit `config.yaml` while z-web is running.
- Adding a server entry → starts it immediately
- Removing a server entry → stops it gracefully
- No restart needed

## Endpoints

| Endpoint          | Description                         |
|-------------------|-------------------------------------|
| `GET /healthz`    | Health check (JSON)                 |
| `GET /livereload` | WebSocket hot reload connection     |
| `GET /*`          | Static file serving                 |

## Logging

```bash
RUST_LOG=debug ./z-web       # verbose
RUST_LOG=z_web=warn ./z-web  # warnings only
```

## Features

- **Config hot reload** — add/remove servers without restart
- **Multi-server** — run N servers from one process
- **File hot reload** — browser auto-refreshes on file change (WebSocket)
- **Gzip compression** — automatic for text assets
- **Health endpoint** — `/healthz` for uptime monitoring
- **Graceful shutdown** — Ctrl+C / SIGTERM safe
- **Smart watching** — only real file changes trigger reload (not reads)
- **Config validation** — duplicate ports are rejected before servers start; one bad
  entry no longer crashes the whole process

## Development

```bash
cargo test              # run unit tests
cargo clippy -- -D warnings
cargo fmt
```

## Contributing

Issues and pull requests are welcome. Please run `cargo fmt` and `cargo clippy`
before submitting a PR.

## License

MIT — see [LICENSE](LICENSE).

## Support

If z-web is useful to you, you can support development at
[trakteer.id/zacxyonly](https://trakteer.id/zacxyonly).
