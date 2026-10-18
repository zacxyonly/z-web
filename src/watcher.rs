use notify::{
    event::{EventKind, ModifyKind},
    Event, RecursiveMode, Watcher,
};
use std::path::Path;
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Spawn a recursive file watcher on `folder`.
/// Broadcasts on `tx` only for actual content changes (not reads/metadata).
/// Ignores temporary editor files and hidden paths (.git, etc.).
/// Caller must hold the returned Watcher — dropping it stops watching.
pub fn spawn_watcher(
    folder: &str,
    tx: broadcast::Sender<()>,
) -> Result<impl Watcher, notify::Error> {
    let folder_owned = folder.to_string();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) if should_reload(&event) && !is_ignored_path(&event) => {
                debug!(folder = %folder_owned, kind = ?event.kind, "Change → reload");
                let _ = tx.send(());
            }
            Ok(_) => {}
            Err(e) => warn!("Watcher error: {e}"),
        })?;

    watcher.watch(Path::new(folder), RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// Spawn a watcher specifically for config.yaml.
/// Sends a signal on `tx` when the config file changes.
/// Returns a Watcher that must be kept alive by the caller.
pub fn spawn_config_watcher(
    config_path: &str,
    tx: tokio::sync::mpsc::Sender<()>,
) -> Result<impl Watcher, notify::Error> {
    let label = config_path.to_string();
    let path = Path::new(config_path);
    let target_filename = path
        .file_name()
        .map(|f| f.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("config.yaml"));

    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event)
                if should_reload(&event)
                    && (event.paths.is_empty()
                        || event
                            .paths
                            .iter()
                            .any(|p| p.file_name() == Some(&target_filename))) =>
            {
                debug!(file = %label, kind = ?event.kind, "config.yaml changed");
                let _ = tx.try_send(());
            }
            Ok(_) => {}
            Err(e) => warn!("Config watcher error: {e}"),
        })?;

    // Watch parent dir so we catch atomic writes (editor write+rename)
    let watch_target = path.parent().unwrap_or(Path::new("."));
    let watch_target = if watch_target.as_os_str().is_empty() {
        Path::new(".")
    } else {
        watch_target
    };
    watcher.watch(watch_target, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn should_reload(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_) | ModifyKind::Any)
    )
}

fn is_ignored_path(event: &Event) -> bool {
    !event.paths.is_empty()
        && event.paths.iter().all(|p| {
            p.components().any(|c| {
                if let std::path::Component::Normal(os_str) = c {
                    let name = os_str.to_string_lossy();
                    name.starts_with('.') || name.ends_with('~') || name.ends_with(".tmp")
                } else {
                    false
                }
            })
        })
}
