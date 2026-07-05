use notify::{
    event::{EventKind, ModifyKind},
    Event, RecursiveMode, Watcher,
};
use std::path::Path;
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Spawn a recursive file watcher on `folder`.
/// Broadcasts on `tx` only for actual content changes (not reads/metadata).
/// Caller must hold the returned Watcher — dropping it stops watching.
pub fn spawn_watcher(
    folder: &str,
    tx: broadcast::Sender<()>,
) -> Result<impl Watcher, notify::Error> {
    let folder_owned = folder.to_string();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) if should_reload(&event) => {
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
/// Sends a signal on `tx` when the file changes.
/// Returns a boxed Watcher that must be kept alive by the caller.
pub fn spawn_config_watcher(
    config_path: &str,
    tx: tokio::sync::mpsc::Sender<()>,
) -> Result<impl Watcher, notify::Error> {
    let label = config_path.to_string();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) if should_reload(&event) => {
                debug!(file = %label, kind = ?event.kind, "config.yaml changed");
                let _ = tx.try_send(());
            }
            Ok(_) => {}
            Err(e) => warn!("Config watcher error: {e}"),
        })?;

    // Watch parent dir so we catch atomic writes (editor write+rename)
    let path = Path::new(config_path);
    let watch_target = path.parent().unwrap_or(Path::new("."));
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
