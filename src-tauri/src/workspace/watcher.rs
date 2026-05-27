use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

use crate::error::Result;

/// Events emitted by the FileWatcher.
#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
}

/// Directories to exclude from watching.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    ".cache",
    "__pycache__",
    ".venv",
    "venv",
    ".svelte-kit",
];

/// FileWatcher monitors a directory for file system changes.
///
/// Uses the `notify` crate with a blocking thread bridged to tokio's async
/// event channel. Excluded directories (`.git`, `node_modules`, etc.) are
/// filtered out before forwarding events.
pub struct FileWatcher {
    event_tx: mpsc::Sender<FileChangeEvent>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl FileWatcher {
    /// Create a new FileWatcher for the given root path.
    ///
    /// Spawns a background blocking thread that watches the filesystem and
    /// forwards [`FileChangeEvent`]s to the returned receiver.
    pub fn spawn(root: &Path) -> Result<(Self, mpsc::Receiver<FileChangeEvent>)> {
        let (event_tx, event_rx) = mpsc::channel::<FileChangeEvent>(256);
        let watch_root = root.to_path_buf();
        let tx = event_tx.clone();

        let blocking_handle = tokio::task::spawn_blocking(move || {
            Self::run_blocking(&watch_root, tx);
        });

        // Wrap so the outer JoinHandle can be aborted on drop
        let handle = tokio::spawn(async move {
            let _ = blocking_handle.await;
        });

        Ok((
            Self {
                event_tx,
                handle: Some(handle),
            },
            event_rx,
        ))
    }

    /// Blocking loop that owns the `notify` watcher and forwards events.
    fn run_blocking(root: &Path, tx: mpsc::Sender<FileChangeEvent>) {
        use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

        let (notify_tx, notify_rx) = std::sync::mpsc::channel::<
            std::result::Result<notify::Event, notify::Error>,
        >();

        let mut watcher = match RecommendedWatcher::new(
            move |res| {
                let _ = notify_tx.send(res);
            },
            Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
            log::error!("Failed to start watching '{}': {}", root.display(), e);
            return;
        }

        log::info!("FileWatcher started on '{}'", root.display());

        loop {
            match notify_rx.recv() {
                Ok(Ok(event)) => {
                    let path = match event.paths.first() {
                        Some(p) => p.clone(),
                        None => continue,
                    };

                    let change = match event.kind {
                        EventKind::Create(_) => FileChangeEvent::Created(path),
                        EventKind::Modify(_) => FileChangeEvent::Modified(path),
                        EventKind::Remove(_) => FileChangeEvent::Deleted(path),
                        _ => continue,
                    };

                    if Self::is_excluded(&change) {
                        continue;
                    }

                    if tx.blocking_send(change).is_err() {
                        log::warn!("FileWatcher event channel closed");
                        break;
                    }
                }
                Ok(Err(e)) => {
                    log::warn!("FileWatcher notify error: {}", e);
                }
                Err(_) => {
                    log::info!("FileWatcher channel disconnected");
                    break;
                }
            }
        }
    }

    /// Returns `true` if the event's path lies inside an excluded directory.
    fn is_excluded(event: &FileChangeEvent) -> bool {
        let path = match event {
            FileChangeEvent::Created(p)
            | FileChangeEvent::Modified(p)
            | FileChangeEvent::Deleted(p) => p,
        };

        path.components().any(|comp| {
            if let std::path::Component::Normal(name) = comp {
                if let Some(s) = name.to_str() {
                    return EXCLUDED_DIRS.contains(&s);
                }
            }
            false
        })
    }

    /// Get a sender for subscribing to events (clone of the internal channel).
    pub fn subscribe(&self) -> mpsc::Sender<FileChangeEvent> {
        self.event_tx.clone()
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_is_excluded_node_modules() {
        let event =
            FileChangeEvent::Created(PathBuf::from("/project/node_modules/pkg/index.js"));
        assert!(FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_is_excluded_git() {
        let event = FileChangeEvent::Modified(PathBuf::from("/project/.git/HEAD"));
        assert!(FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_is_excluded_target() {
        let event = FileChangeEvent::Created(PathBuf::from("/project/target/debug/app"));
        assert!(FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_is_excluded_svelte_kit() {
        let event = FileChangeEvent::Created(PathBuf::from("/project/.svelte-kit/generated"));
        assert!(FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_not_excluded_src_file() {
        let event = FileChangeEvent::Modified(PathBuf::from("/project/src/main.rs"));
        assert!(!FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_not_excluded_deep_src() {
        let event =
            FileChangeEvent::Deleted(PathBuf::from("/project/src/components/Button.tsx"));
        assert!(!FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_not_excluded_cargo_toml() {
        let event = FileChangeEvent::Modified(PathBuf::from("/project/Cargo.toml"));
        assert!(!FileWatcher::is_excluded(&event));
    }

    #[test]
    fn test_subscribe_clone() {
        let (tx, _rx) = mpsc::channel::<FileChangeEvent>(256);
        let watcher = FileWatcher {
            event_tx: tx,
            handle: None,
        };

        let tx2 = watcher.subscribe();
        let event = FileChangeEvent::Created(PathBuf::from("/test/file.rs"));
        // Both the original tx and the subscribed tx should work
        assert!(tx2.try_send(event).is_ok());
    }
}
