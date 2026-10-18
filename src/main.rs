mod config;
mod php;
mod server;
mod watcher;

use std::{collections::HashMap, sync::Arc};
use tokio::{signal, sync::broadcast};
use tracing::{error, info, warn};

use config::{Config, ServerConfig, CONFIG_FILE};
use server::build_app;
use watcher::{spawn_config_watcher, spawn_watcher};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "-v" | "--version" => {
                println!("z-web v{}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                println!("z-web v{}", env!("CARGO_PKG_VERSION"));
                println!(
                    "Lightweight multi-server static file server with hot reload and PHP support\n"
                );
                println!("Usage: z-web [OPTIONS]\n");
                println!("Options:");
                println!("  -v, --version    Print version information");
                println!("  -h, --help       Print help information");
                return;
            }
            _ => {}
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "z_web=info".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    if config.servers.is_empty() {
        warn!("No servers configured in config.yaml.");
        std::process::exit(0);
    }

    info!("z-web v{} starting", env!("CARGO_PKG_VERSION"));

    // Channel for config.yaml change notifications
    let (cfg_tx, mut cfg_rx) = tokio::sync::mpsc::channel::<()>(4);

    // Keep config watcher alive for the duration of the program
    let _config_watcher = match spawn_config_watcher(CONFIG_FILE, cfg_tx) {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to watch config.yaml: {e}");
            std::process::exit(1);
        }
    };

    // State: map of port → ServerInstance
    let mut running: HashMap<u16, ServerInstance> = HashMap::new();

    // Initial server spawn
    spawn_servers(&config.servers, &mut running).await;

    info!("All servers running. Edit config.yaml to add/remove servers. Ctrl+C to stop.");

    loop {
        tokio::select! {
            // config.yaml changed
            Some(_) = cfg_rx.recv() => {
                // Debounce: drain any rapid successive events
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                while cfg_rx.try_recv().is_ok() {}

                info!("config.yaml changed, reloading...");
                match Config::reload() {
                    Err(e) => {
                        warn!("Config reload failed (keeping current servers): {e}");
                    }
                    Ok(new_cfg) => {
                        let new_ports: std::collections::HashSet<u16> =
                            new_cfg.servers.iter().map(|s| s.port).collect();
                        let old_ports: std::collections::HashSet<u16> =
                            running.keys().copied().collect();

                        // 1. Stop removed servers
                        for port in old_ports.difference(&new_ports).copied().collect::<Vec<_>>() {
                            if let Some(instance) = running.remove(&port) {
                                info!(port, "Stopping removed server");
                                let _ = instance.shutdown_tx.send(());
                            }
                        }

                        // 2. Restart servers whose configuration has changed on the same port
                        for new_s in &new_cfg.servers {
                            if let Some(instance) = running.get(&new_s.port) {
                                if instance.config != *new_s {
                                    info!(port = new_s.port, "Server configuration changed, restarting...");
                                    if let Some(old) = running.remove(&new_s.port) {
                                        let _ = old.shutdown_tx.send(());
                                    }
                                }
                            }
                        }

                        // 3. Start new or restarted servers
                        let to_start: Vec<ServerConfig> = new_cfg
                            .servers
                            .into_iter()
                            .filter(|s| !running.contains_key(&s.port))
                            .collect();

                        if !to_start.is_empty() {
                            spawn_servers(&to_start, &mut running).await;
                        }
                    }
                }
            }

            // Ctrl+C / SIGTERM
            _ = shutdown_signal() => {
                info!("Shutting down all servers.");
                break;
            }
        }
    }
}

struct ServerInstance {
    config: ServerConfig,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    _watcher: notify::RecommendedWatcher,
}

/// Spawn servers for the given configs, inserting into `running`.
async fn spawn_servers(
    servers: &[ServerConfig],
    running: &mut HashMap<u16, ServerInstance>,
) {
    for server_cfg in servers {
        let port = server_cfg.port;

        if running.contains_key(&port) {
            warn!(port, "Server already running on this port, skipping");
            continue;
        }

        if let Err(e) = server_cfg.ensure_folder() {
            error!(folder = %server_cfg.web_folder, "Failed to prepare web folder: {e}");
            continue;
        }

        let addr = match server_cfg.socket_addr() {
            Ok(a) => a,
            Err(e) => {
                error!("Skipping server on port {port}: {e}");
                continue;
            }
        };

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!(addr = %addr, "Bind failed: {e}");
                continue;
            }
        };

        let (reload_tx, _) = broadcast::channel::<()>(64);
        let state = Arc::new(server::AppState {
            tx: reload_tx.clone(),
        });

        let file_watcher = match spawn_watcher(&server_cfg.web_folder, reload_tx) {
            Ok(w) => w,
            Err(e) => {
                error!(folder = %server_cfg.web_folder, "Watcher failed: {e}");
                continue;
            }
        };

        let app = build_app(state, server_cfg);
        let folder = server_cfg.web_folder.clone();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            info!(addr = %addr, folder = %folder, "Server listening");
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
            info!(addr = %addr, "Server stopped");
        });

        running.insert(
            port,
            ServerInstance {
                config: server_cfg.clone(),
                shutdown_tx,
                _watcher: file_watcher,
            },
        );
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("Ctrl+C handler failed");
    };

    #[cfg(unix)]
    {
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler failed")
                .recv()
                .await;
        };
        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}
