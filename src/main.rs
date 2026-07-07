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

    // State: map of port → (shutdown_tx, file_watcher)
    // shutdown_tx is a oneshot used to kill that server instance
    let mut running: HashMap<u16, (tokio::sync::oneshot::Sender<()>, Box<dyn std::any::Any + Send>)> = HashMap::new();

    // Initial server spawn
    spawn_servers(&config.servers, &mut running);

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

                        // Stop removed servers
                        for port in old_ports.difference(&new_ports).copied().collect::<Vec<_>>() {
                            if let Some((shutdown_tx, _)) = running.remove(&port) {
                                info!(port, "Stopping removed server");
                                let _ = shutdown_tx.send(());
                            }
                        }

                        // Start new servers only (existing ports are left untouched)
                        let new_servers: Vec<ServerConfig> = new_cfg
                            .servers
                            .into_iter()
                            .filter(|s| !old_ports.contains(&s.port))
                            .collect();

                        if !new_servers.is_empty() {
                            spawn_servers(&new_servers, &mut running);
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

/// Spawn servers for the given configs, inserting into `running`.
fn spawn_servers(
    servers: &[ServerConfig],
    running: &mut HashMap<u16, (tokio::sync::oneshot::Sender<()>, Box<dyn std::any::Any + Send>)>,
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

        let (reload_tx, _) = broadcast::channel::<()>(64);
        let state = Arc::new(server::AppState {
            tx: reload_tx.clone(),
            cfg: server_cfg.clone(),
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
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!(addr = %addr, "Bind failed: {e}");
                    return;
                }
            };
            info!(addr = %addr, folder = %folder, "Server listening");
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
            info!(addr = %addr, "Server stopped");
        });

        running.insert(port, (shutdown_tx, Box::new(file_watcher)));
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

