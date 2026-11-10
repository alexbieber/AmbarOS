//! Trigger sources: schedules and folder watching.
//!
//! Each source runs as a task that sends [`Fire`] events to the engine until
//! its cancellation token is triggered.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use globset::{Glob, GlobMatcher};
use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::error::{EngineError, Result};
use crate::workflow::{FileEventKind, Schedule};

/// One trigger firing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fire {
    /// A schedule tick.
    Schedule,
    /// A settled file event.
    File { path: PathBuf, kind: FileEventKind },
}

/// Spawns a schedule source.
pub(crate) fn spawn_schedule(
    schedule: Schedule,
    tx: mpsc::Sender<Fire>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match schedule {
            Schedule::Every(period) => {
                let mut ticker = tokio::time::interval_at(Instant::now() + period, period);
                ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = ticker.tick() => {
                            if tx.send(Fire::Schedule).await.is_err() { break; }
                        }
                    }
                }
            }
            Schedule::Cron(cron) => loop {
                let now = chrono::Local::now();
                let next = match cron.find_next_occurrence(&now, false) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!(error = %e, "cron schedule has no next occurrence");
                        break;
                    }
                };
                let wait = (next - now).to_std().unwrap_or(Duration::ZERO);
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(wait) => {
                        if tx.send(Fire::Schedule).await.is_err() { break; }
                    }
                }
            },
        }
    })
}

/// Settings for a folder watch.
#[derive(Debug, Clone)]
pub(crate) struct WatchSpec {
    pub folder: PathBuf,
    pub events: Vec<FileEventKind>,
    pub pattern: Option<String>,
    pub recursive: bool,
    /// A file must be quiet this long before its event fires.
    pub debounce: Duration,
}

fn classify(kind: &EventKind, path: &Path, index: usize, count: usize) -> Option<FileEventKind> {
    match kind {
        EventKind::Create(_) => Some(FileEventKind::Created),
        EventKind::Remove(_) => Some(FileEventKind::Removed),
        EventKind::Modify(ModifyKind::Name(mode)) => match mode {
            RenameMode::To => Some(FileEventKind::Created),
            RenameMode::From => Some(FileEventKind::Removed),
            RenameMode::Both if count == 2 => Some(if index == 0 {
                FileEventKind::Removed
            } else {
                FileEventKind::Created
            }),
            // Platforms that do not say which side of a rename this is.
            _ => Some(if path.exists() {
                FileEventKind::Created
            } else {
                FileEventKind::Removed
            }),
        },
        EventKind::Modify(ModifyKind::Metadata(_)) => None,
        EventKind::Modify(_) => Some(FileEventKind::Modified),
        _ => None,
    }
}

/// Starts watching a folder. Fails immediately if the folder cannot be watched.
pub(crate) fn spawn_folder_watch(
    spec: WatchSpec,
    tx: mpsc::Sender<Fire>,
    cancel: CancellationToken,
) -> Result<JoinHandle<()>> {
    if !spec.folder.is_dir() {
        return Err(EngineError::Trigger(format!(
            "watched folder {} does not exist",
            spec.folder.display()
        )));
    }
    let matcher: Option<GlobMatcher> = spec
        .pattern
        .as_deref()
        .map(|p| Glob::new(p).map(|g| g.compile_matcher()))
        .transpose()
        .map_err(|e| EngineError::Trigger(e.to_string()))?;

    let (raw_tx, mut raw_rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res| {
        let _ = raw_tx.send(res);
    })
    .map_err(|e| EngineError::Trigger(e.to_string()))?;
    let mode = if spec.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    watcher
        .watch(&spec.folder, mode)
        .map_err(|e| EngineError::Trigger(e.to_string()))?;
    // Paths in events are canonical on some platforms (macOS /private/...).
    let folder_canon = spec
        .folder
        .canonicalize()
        .unwrap_or_else(|_| spec.folder.clone());

    Ok(tokio::spawn(async move {
        let _watcher = watcher; // keep alive for the task's lifetime
        let mut pending: HashMap<PathBuf, (FileEventKind, Instant)> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                Some(res) = raw_rx.recv() => {
                    let event = match res {
                        Ok(e) => e,
                        Err(e) => { tracing::warn!(error = %e, "folder watch error"); continue; }
                    };
                    let count = event.paths.len();
                    for (i, path) in event.paths.iter().enumerate() {
                        let Some(kind) = classify(&event.kind, path, i, count) else { continue };
                        let in_scope = if spec.recursive {
                            path.starts_with(&spec.folder) || path.starts_with(&folder_canon)
                        } else {
                            path.parent().is_some_and(|p| p == spec.folder || p == folder_canon)
                        };
                        if !in_scope { continue; }
                        let name_ok = match (&matcher, path.file_name()) {
                            (Some(m), Some(name)) => m.is_match(name),
                            (Some(_), None) => false,
                            (None, _) => true,
                        };
                        if !name_ok { continue; }
                        // A new file that is then written is still "created".
                        let kind = match pending.get(path) {
                            Some((FileEventKind::Created, _)) if kind == FileEventKind::Modified => FileEventKind::Created,
                            _ => kind,
                        };
                        pending.insert(path.clone(), (kind, Instant::now()));
                    }
                }
                _ = tick.tick() => {
                    let now = Instant::now();
                    let ready: Vec<PathBuf> = pending
                        .iter()
                        .filter(|(_, (_, seen))| now.duration_since(*seen) >= spec.debounce)
                        .map(|(p, _)| p.clone())
                        .collect();
                    for path in ready {
                        let Some((kind, _)) = pending.remove(&path) else { continue };
                        // Ignore directories and files that vanished before settling.
                        if kind != FileEventKind::Removed && !path.is_file() { continue; }
                        if !spec.events.contains(&kind) { continue; }
                        if tx.send(Fire::File { path, kind }).await.is_err() { return; }
                    }
                }
            }
        }
    }))
}
