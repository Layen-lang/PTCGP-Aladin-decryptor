// aladin-app/src/worker.rs

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use aladin_core::{
    adb::{list_remote_blob_stems, pull_directory, pull_file, REMOTE_BASE},
    pipeline::{run_pipeline, PipelineEvent},
    state::ProcessingState,
};

#[derive(Debug, Clone)]
pub enum WorkerMsg {
    /// ADB pull progress.
    /// - Bulk (first run): current = percentage 0–100, total = 100
    /// - Incremental:      current = files downloaded, total = new file count
    PullProgress { current: usize, total: usize },
    PipelineEvent(PipelineEvent),
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerAction {
    PullOnly,
    DecryptOnly,
    Full, // Keep Full for backward compatibility if needed, though we will use separate buttons
}

pub fn start_worker(
    serial: String,
    output_dir: PathBuf,
    pull_dir: PathBuf,
    action: WorkerAction,
) -> Receiver<WorkerMsg> {
    let (tx, rx) = mpsc::channel::<WorkerMsg>();
    thread::spawn(move || {
        worker_thread(serial, output_dir, pull_dir, action, tx);
    });
    rx
}

fn worker_thread(
    serial: String,
    output_dir: PathBuf,
    pull_dir: PathBuf,
    action: WorkerAction,
    tx: Sender<WorkerMsg>,
) {
    // cache_base = <pull_dir>/files/  (created by adb pull REMOTE_BASE <pull_dir>)
    let cache_base = pull_dir.join("files");

    if action == WorkerAction::PullOnly || action == WorkerAction::Full {
        // ── 1. Pull ────────────────────────────────────────────────────────────────
        let blob_dir = cache_base.join("Sharin.Resources/Default/blob");
        let has_cache = blob_dir.exists()
            && std::fs::read_dir(&blob_dir)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false);

        if has_cache {
            // ── Incremental pull ───────────────────────────────────────────────────
            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(
                "[→] Local cache found — checking for new files…".into(),
            )));

            // Always re-pull DefaultMasterData and index (small, may change each update)
            if let Err(e) = refresh_support_dirs(&serial, &cache_base, &tx) {
                let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(e)));
                let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Done {
                    decrypted: 0,
                    errors: 1,
                }));
                return;
            }

            // New blobs
            let remote_stems = match list_remote_blob_stems(&serial) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(
                        format!("[✗] ADB list: {e}"),
                    )));
                    let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Done {
                        decrypted: 0,
                        errors: 1,
                    }));
                    return;
                }
            };

            let cached: HashSet<String> = scan_local_blob_stems(&cache_base).into_iter().collect();
            let to_pull: Vec<String> = remote_stems
                .into_iter()
                .filter(|s| !cached.contains(s))
                .collect();

            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(format!(
                "[→] {} new blobs to download",
                to_pull.len()
            ))));

            if !to_pull.is_empty() {
                let total = to_pull.len();
                let _ = tx.send(WorkerMsg::PullProgress { current: 0, total });

                for (i, stem) in to_pull.iter().enumerate() {
                    let prefix = if stem.len() >= 2 { &stem[..2] } else { stem.as_str() };
                    let remote = format!(
                        "{}/Sharin.Resources/Default/blob/{}/{}.aladin",
                        REMOTE_BASE, prefix, stem
                    );
                    let local = cache_base.join(format!(
                        "Sharin.Resources/Default/blob/{}/{}.aladin",
                        prefix, stem
                    ));
                    if let Err(e) = pull_file(&serial, &remote, &local, &|_| {}) {
                        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(
                            format!("[!] Pull {stem}: {e}"),
                        )));
                    }
                    let _ = tx.send(WorkerMsg::PullProgress { current: i + 1, total });
                }
            }
        } else {
            // ── First run: full pull with file-counting progress ──────────────────
            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(
                "[→] First full pull of the directory…".into(),
            )));

            // 1. Get total expected count
            let remote_stems = list_remote_blob_stems(&serial).unwrap_or_default();
            let total = remote_stems.len();
            let _ = tx.send(WorkerMsg::PullProgress { current: 0, total });

            // 2. Start pull in a separate thread
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let serial_clone = serial.clone();
            let pull_dir_clone = pull_dir.clone();
            let tx_log = tx.clone();
            std::thread::spawn(move || {
                let log = |s: String| {
                    let _ = tx_log.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(s)));
                };
                let res = aladin_core::adb::pull_directory(&serial_clone, REMOTE_BASE, &pull_dir_clone, &log);
                let _ = done_tx.send(res);
            });

            // 3. Watch the directory while pulling
            loop {
                match done_rx.try_recv() {
                    Ok(res) => {
                        if let Err(e) = res {
                            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(
                                format!("[✗] ADB pull: {e}"),
                            )));
                            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Done {
                                decrypted: 0,
                                errors: 1,
                            }));
                            return;
                        }
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // Count local files
                        let current = scan_local_blob_stems(&cache_base).len();
                        let _ = tx.send(WorkerMsg::PullProgress { current, total });
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(
            "[✓] Pull complete".into(),
        )));
        let _ = tx.send(WorkerMsg::PullProgress { current: 100, total: 100 });
    }

    if action == WorkerAction::DecryptOnly || action == WorkerAction::Full {
        // ── 2. Filter blobs not yet decrypted ────────────────────────────────────
        let state = ProcessingState::load(&output_dir);
        let new_stems: Vec<String> = scan_local_blob_stems(&cache_base)
            .into_iter()
            .filter(|stem| !state.is_processed(stem))
            .collect();

        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(format!(
            "[→] {} blobs to decrypt",
            new_stems.len()
        ))));

        // ── 3. Decryption pipeline ────────────────────────────────────────────────
        let (pipe_tx, pipe_rx) = std::sync::mpsc::channel();
        let cache_clone = cache_base.clone();
        let out_clone = output_dir.clone();
        let stems_clone = new_stems.clone();
        thread::spawn(move || {
            run_pipeline(&cache_clone, &out_clone, &stems_clone, pipe_tx);
        });

        for event in pipe_rx {
            let _ = tx.send(WorkerMsg::PipelineEvent(event));
        }
    } else {
        // Just pull was requested, finish here
        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Done {
            decrypted: 0,
            errors: 0,
        }));
    }
}

/// Re-pulls DefaultMasterData and the full index from the device.
/// These directories are small and change with every game update.
/// Returns an error string on the first failure.
fn refresh_support_dirs(serial: &str, cache_base: &Path, tx: &Sender<WorkerMsg>) -> Result<(), String> {
    let log = |s: String| {
        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(s)));
    };

    let remote_master = format!("{}/DefaultMasterData", REMOTE_BASE);
    pull_directory(serial, &remote_master, cache_base, &log)
        .map_err(|e| format!("[✗] Pull DefaultMasterData: {e}"))?;

    let remote_index = format!("{}/Sharin.Resources/Default/index", REMOTE_BASE);
    let index_parent = cache_base.join("Sharin.Resources/Default");
    pull_directory(serial, &remote_index, &index_parent, &log)
        .map_err(|e| format!("[✗] Pull index: {e}"))?;

    Ok(())
}

/// Scans blob stems from the local Default cache.
fn scan_local_blob_stems(cache_base: &Path) -> Vec<String> {
    let blob_dir = cache_base.join("Sharin.Resources/Default/blob");
    let mut stems = Vec::new();
    let Ok(subdirs) = std::fs::read_dir(&blob_dir) else { return stems };
    for subdir in subdirs.flatten() {
        let Ok(files) = std::fs::read_dir(subdir.path()) else { continue };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().map(|e| e == "aladin").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    stems.push(stem.to_string());
                }
            }
        }
    }
    stems
}
