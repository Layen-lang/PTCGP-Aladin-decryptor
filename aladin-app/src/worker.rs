// aladin-app/src/worker.rs

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use aladin_core::{
    adb::{list_remote_blob_stems, pull_file, pull_directory, REMOTE_BASE},
    pipeline::{run_pipeline, PipelineEvent, NAMESPACES},
    state::ProcessingState,
};

#[derive(Debug, Clone)]
pub enum WorkerMsg {
    /// ADB pull progress.
    /// - Bulk (first run): current = percentage 0–100, total = 100
    /// - Incremental:      current = files downloaded, total = new file count (cross-namespace)
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
    // cache_base = <pull_dir> (now directly the .cache folder)
    let cache_base = pull_dir;

    if action == WorkerAction::PullOnly || action == WorkerAction::Full {
        // ── 1. Pull APKs ───────────────────────────────────────────────────────────
        if let Err(e) = pull_and_extract_apks(&serial, &cache_base, &tx) {
            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(format!("[✗] APK processing: {e}"))));
        }

        // ── 2. Pull Blobs ──────────────────────────────────────────────────────────
        // Per-namespace incremental blob pull
        let mut to_pull_per_ns: Vec<(&'static str, String, String, Vec<String>)> = Vec::new();
        
        // Always re-pull DefaultMasterData and indexes (small, may change each update)
        if let Err(e) = refresh_support_dirs(&serial, &cache_base, &tx) {
            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(e)));
        }

        for ns in NAMESPACES {
            // AssetPack and Data are handled by APK pull, skip ADB list for them
            if ns.name == "AssetPack" || ns.name == "Data" { continue; }

            // Map clean local dir to remote complex path
            let remote_blob_rel = format!("Sharin.Resources/{}/blob", ns.name);
            let local_blob_dir = cache_base.join(ns.blob_dir_rel);

            // Optimization: if local directory doesn't exist or is empty, pull the whole folder
            let is_empty = !local_blob_dir.exists() || 
                           std::fs::read_dir(&local_blob_dir).map(|mut d| d.next().is_none()).unwrap_or(true);

            if is_empty {
                let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(format!(
                    "[→] {} — performing full folder pull (Solution B)…",
                    ns.name
                ))));
                let log = |s: String| {
                    let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(s)));
                };
                let remote_path = format!("{}/{}", REMOTE_BASE, remote_blob_rel);
                
                // Ensure local_blob_dir's parent exists, then pull to it
                if let Some(parent) = local_blob_dir.parent() {
                    std::fs::create_dir_all(parent).ok();
                    if let Err(e) = pull_directory(&serial, &remote_path, parent, &log) {
                        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(format!("[✗] Full pull {}: {e}", ns.name))));
                    }
                }
            } else {
                // Incremental pull: list remote and compare with local
                let remote_stems = match list_remote_blob_stems(&serial, &remote_blob_rel) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(
                            format!("[✗] ADB list {}: {e}", ns.name),
                        )));
                        continue;
                    }
                };

                let cached: HashSet<String> = scan_local_blob_stems(&cache_base, ns.blob_dir_rel)
                    .into_iter()
                    .collect();
                let to_pull: Vec<String> = remote_stems
                    .into_iter()
                    .filter(|s| !cached.contains(s))
                    .collect();

                if !to_pull.is_empty() {
                    let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(format!(
                        "[→] {} — {} new blobs to download",
                        ns.name,
                        to_pull.len()
                    ))));
                    to_pull_per_ns.push((ns.name, ns.blob_dir_rel.to_string(), remote_blob_rel, to_pull));
                }
            }
        }

        // Process incremental pulls
        let total: usize = to_pull_per_ns.iter().map(|(_, _, _, v)| v.len()).sum();
        if total > 0 {
            let _ = tx.send(WorkerMsg::PullProgress { current: 0, total });

            let mut pulled = 0usize;
            for (_ns_name, local_blob_rel, remote_blob_rel, stems) in &to_pull_per_ns {
                for stem in stems {
                    let prefix = if stem.len() >= 2 { &stem[..2] } else { stem.as_str() };
                    let remote = format!(
                        "{}/{}/{}/{}.aladin",
                        REMOTE_BASE, remote_blob_rel, prefix, stem
                    );
                    let local = cache_base.join(format!(
                        "{}/{}/{}.aladin",
                        local_blob_rel, prefix, stem
                    ));
                    if let Err(e) = pull_file(&serial, &remote, &local, &|_| {}) {
                        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(
                            format!("[!] Pull {stem}: {e}"),
                        )));
                    }
                    pulled += 1;
                    let _ = tx.send(WorkerMsg::PullProgress { current: pulled, total });
                }
            }
        }
        
        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(
            "[✓] Pull complete".into(),
        )));
        let _ = tx.send(WorkerMsg::PullProgress { current: 100, total: 100 });
    }

    if action == WorkerAction::DecryptOnly || action == WorkerAction::Full {
        // ── 3. Build per-namespace stem lists, filtered against state.json ────────
        let state = ProcessingState::load(&output_dir);
        let mut new_stems: HashMap<String, Vec<String>> = HashMap::new();
        for ns in NAMESPACES {
            let stems: Vec<String> = scan_local_blob_stems(&cache_base, ns.blob_dir_rel)
                .into_iter()
                .filter(|stem| !state.is_processed(ns.name, stem))
                .collect();
            let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(format!(
                "[→] {} — {} blobs to decrypt",
                ns.name,
                stems.len()
            ))));
            new_stems.insert(ns.name.to_string(), stems);
        }

        // ── 4. Decryption pipeline ────────────────────────────────────────────────
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

/// Re-pulls DefaultMasterData and the index of every namespace.
fn refresh_support_dirs(serial: &str, cache_base: &Path, tx: &Sender<WorkerMsg>) -> Result<(), String> {
    let log = |s: String| {
        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(s)));
    };

    let remote_master = format!("{}/DefaultMasterData", REMOTE_BASE);
    // Pull DefaultMasterData into cache_base, it will create cache_base/DefaultMasterData
    pull_directory(serial, &remote_master, cache_base, &log)
        .map_err(|e| format!("[✗] Pull DefaultMasterData: {e}"))?;

    for ns in NAMESPACES {
        if ns.index_dir_rel.is_empty() { continue; }
        // AssetPack is in APK, handled there
        if ns.name == "AssetPack" { continue; }

        let remote_index = format!("{}/Sharin.Resources/{}/index", REMOTE_BASE, ns.name);
        // Pull remote index folder into local namespace folder to get ns_name/index
        let local_ns_dir = cache_base.join(ns.name);
        std::fs::create_dir_all(&local_ns_dir).ok();
        if let Err(e) = pull_directory(serial, &remote_index, &local_ns_dir, &log) {
             let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Error(format!("[✗] Pull {} index: {e}", ns.name))));
        }
    }

    Ok(())
}

fn pull_and_extract_apks(serial: &str, cache_base: &Path, tx: &Sender<WorkerMsg>) -> Result<(), String> {
    let log = |s: String| {
        let _ = tx.send(WorkerMsg::PipelineEvent(PipelineEvent::Log(s)));
    };

    let pkg = "jp.pokemon.pokemontcgp";
    let apk_paths = aladin_core::adb::get_package_apk_paths(serial, pkg)?;
    
    let tmp_apk_dir = cache_base.join("tmp_apks");
    // Always delete and recreate the APK directory to ensure fresh pull
    if tmp_apk_dir.exists() {
        std::fs::remove_dir_all(&tmp_apk_dir).ok();
    }
    std::fs::create_dir_all(&tmp_apk_dir).ok();

    for remote_path in apk_paths {
        let filename = remote_path.rsplit('/').next().unwrap_or("app.apk");
        if filename != "base.apk" && filename != "split_bundledtree.apk" {
            continue;
        }

        let local_apk = tmp_apk_dir.join(filename);
        
        log(format!("[→] Pulling {}…", filename));
        pull_file(serial, &remote_path, &local_apk, &|_| {})?;

        log(format!("[→] Extracting {}…", filename));
        let file = std::fs::File::open(&local_apk).map_err(|e| format!("open apk: {e}"))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("read zip: {e}"))?;

        let mut extracted_count = 0;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| format!("zip entry {i}: {e}"))?;
            let name = file.name().replace('\\', "/"); // Normalize to forward slashes

            let (target_dir, strip_prefix) = if filename == "base.apk" && name.starts_with("assets/bin/Data/") {
                (cache_base.join("Data"), "assets/bin/Data/")
            } else if filename == "split_bundledtree.apk" && name.starts_with("assets/assetpack/") {
                (cache_base.join("AssetPack"), "assets/assetpack/")
            } else {
                continue;
            };

            let relative_name = name.strip_prefix(strip_prefix).unwrap();
            if relative_name.is_empty() { continue; }
            
            let dest_path = target_dir.join(relative_name.replace('/', std::path::MAIN_SEPARATOR_STR));

            if name.ends_with('/') {
                std::fs::create_dir_all(&dest_path).ok();
            } else {
                if let Some(p) = dest_path.parent() {
                    std::fs::create_dir_all(p).ok();
                }
                let mut outfile = std::fs::File::create(&dest_path).map_err(|e| format!("create {}: {e}", dest_path.display()))?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| format!("copy {}: {e}", dest_path.display()))?;
                extracted_count += 1;
            }
        }
        log(format!("[✓] {} — {} files extracted", filename, extracted_count));
    }

    // Cleanup APKs after extraction
    let _ = std::fs::remove_dir_all(&tmp_apk_dir);

    Ok(())
}

/// Scans blob stems from a local namespace blob directory.
fn scan_local_blob_stems(cache_base: &Path, blob_dir_rel: &str) -> Vec<String> {
    let blob_dir = cache_base.join(blob_dir_rel);
    let mut stems = Vec::new();
    let Ok(entries) = std::fs::read_dir(&blob_dir) else { return stems };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Ok(files) = std::fs::read_dir(path) else { continue };
            for file in files.flatten() {
                let fpath = file.path();
                if fpath.extension().map(|e| e == "aladin").unwrap_or(false) {
                    if let Some(stem) = fpath.file_stem().and_then(|s| s.to_str()) {
                        stems.push(stem.to_string());
                    }
                }
            }
        } else if path.extension().map(|e| e == "aladin").unwrap_or(false) {
            // Support for flat directories like Data
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                stems.push(stem.to_string());
            }
        } else if blob_dir_rel == "Data" {
            // In Data, files might not have extensions
            if let Some(stem) = path.file_name().and_then(|s| s.to_str()) {
                stems.push(stem.to_string());
            }
        }
    }
    stems
}
