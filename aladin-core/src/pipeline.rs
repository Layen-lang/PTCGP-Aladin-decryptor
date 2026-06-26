// aladin-core/src/pipeline.rs
//
// Decryption pipeline:
//   1. Discover and decrypt the main key file (DefaultMasterData/blob/*/*.aladin, 32 bytes)
//   2. For each namespace: discover index files, decrypt + merge, decrypt blobs in parallel.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use rayon::prelude::*;

use crate::{
    ali2::{build_ali2_lookup, Ali2Entry},
    crypto::{decrypt_blob, decrypt_key_file, ALADIN_KEY_BODY, GLOBAL_KEY},
    state::ProcessingState,
};

/// Directory holding the encrypted Default master key (DefaultMasterData/blob/<xx>/<keyIdHash>.aladin).
const KEY_DIR_REL: &str = "DefaultMasterData/blob";

/// How a namespace obtains its 32-byte ChaCha20 key body.
pub enum KeySource {
    /// Decrypt `DefaultMasterData/blob/<xx>/<keyIdHash>.aladin` with the global key.
    DefaultMaster,
    /// Static key embedded in libil2cpp.so (no derivation, no disk file).
    Hardcoded(&'static [u8; 32]),
    /// File is already plain text, no decryption needed.
    Plain,
}

/// Logical groups of blob/index that share the same master key.
pub struct Namespace {
    pub name: &'static str,
    pub blob_dir_rel: &'static str,
    pub index_dir_rel: &'static str,
    pub key: KeySource,
    pub is_flat: bool,
}

pub const NAMESPACES: &[Namespace] = &[
    Namespace {
        name: "Default",
        blob_dir_rel:  "Default/blob",
        index_dir_rel: "Default/index",
        key: KeySource::DefaultMaster,
        is_flat: false,
    },
    Namespace {
        name: "aladin",
        blob_dir_rel:  "aladin/blob",
        index_dir_rel: "aladin/index",
        key: KeySource::Hardcoded(&ALADIN_KEY_BODY),
        is_flat: false,
    },
    Namespace {
        name: "AssetPack",
        blob_dir_rel:  "AssetPack/blob",
        index_dir_rel: "AssetPack/index",
        key: KeySource::DefaultMaster,
        is_flat: false,
    },
    Namespace {
        name: "Data",
        blob_dir_rel:  "Data",
        index_dir_rel: "", // No index for Data
        key: KeySource::Plain,
        is_flat: true,
    },
];

/// Events sent to the UI thread.
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    Log(String),
    Progress { current: usize, total: usize },
    Error(String),
    Done { decrypted: usize, errors: usize },
}

pub type EventSender = std::sync::mpsc::Sender<PipelineEvent>;

/// Runs the full pipeline from `pull_dir` to `output_dir`.
///
/// `new_stems` maps a namespace name (matching `Namespace::name`) to the list
/// of stems to decrypt for that namespace.
pub fn run_pipeline(
    pull_dir: &Path,
    output_dir: &Path,
    new_stems: &HashMap<String, Vec<String>>,
    tx: EventSender,
) {
    // ── 1. Lazily derive the Default master key (only if some namespace needs it) ────
    let needs_default = NAMESPACES.iter().any(|ns| {
        matches!(ns.key, KeySource::DefaultMaster)
            && new_stems.get(ns.name).map(|v| !v.is_empty()).unwrap_or(false)
    });

    let default_kb: Option<[u8; 32]> = if needs_default {
        match find_key_blob(pull_dir).and_then(|(p, id)| {
            let _ = tx.send(PipelineEvent::Log(format!("[→] Key found: {:016x}", id)));
            decrypt_main_key(&p, id)
        }) {
            Ok(k) => {
                let _ = tx.send(PipelineEvent::Log("[✓] Main key decrypted".into()));
                Some(k)
            }
            Err(e) => {
                let _ = tx.send(PipelineEvent::Error(format!("[✗] Main key: {e}")));
                let _ = tx.send(PipelineEvent::Done { decrypted: 0, errors: 1 });
                return;
            }
        }
    } else {
        None
    };

    // ── 2. Setup shared state and global counters ─────────────────────────────
    let total: usize = NAMESPACES
        .iter()
        .map(|ns| new_stems.get(ns.name).map(|v| v.len()).unwrap_or(0))
        .sum();

    let decrypt_dir = output_dir.join("decrypted");
    let decrypted = Arc::new(Mutex::new(0usize));
    let errors    = Arc::new(Mutex::new(0usize));
    let state     = Arc::new(Mutex::new(ProcessingState::load(output_dir)));

    let _ = tx.send(PipelineEvent::Log(format!(
        "[→] Decrypting… ({} threads)",
        rayon::current_num_threads()
    )));

    // ── 3. Per-namespace pipeline ────────────────────────────────────────────
    for ns in NAMESPACES {
        let stems = match new_stems.get(ns.name) {
            Some(v) if !v.is_empty() => v,
            _ => {
                let _ = tx.send(PipelineEvent::Log(format!(
                    "[·] {} — no new blob to decrypt",
                    ns.name
                )));
                continue;
            }
        };

        let kb: [u8; 32] = match &ns.key {
            KeySource::DefaultMaster => match default_kb.as_ref() {
                Some(k) => *k,
                None => {
                    let _ = tx.send(PipelineEvent::Error(format!(
                        "[!] {} — Default master key unavailable, skipping",
                        ns.name
                    )));
                    continue;
                }
            },
            KeySource::Hardcoded(k) => {
                let _ = tx.send(PipelineEvent::Log(format!(
                    "[✓] {} — using embedded key",
                    ns.name
                )));
                **k
            }
            KeySource::Plain => {
                let _ = tx.send(PipelineEvent::Log(format!(
                    "[·] {} — already decrypted",
                    ns.name
                )));
                [0u8; 32] // Unused for Plain
            }
        };

        let index_files = if ns.index_dir_rel.is_empty() {
            Vec::new()
        } else {
            find_all_index_files(pull_dir, ns.index_dir_rel)
        };

        if index_files.is_empty() && !ns.index_dir_rel.is_empty() {
            let _ = tx.send(PipelineEvent::Error(format!(
                "[!] {} — no index file found, skipping",
                ns.name
            )));
            continue;
        }

        let _ = tx.send(PipelineEvent::Log(format!(
            "[→] {} — {} index file(s) to load",
            ns.name,
            index_files.len()
        )));

        let lookup = if ns.index_dir_rel.is_empty() {
            // For Data, we might not have a lookup, but decrypt_one_blob needs one
            // unless we refactor it. For now, let's keep it empty and handle it there.
            HashMap::new()
        } else {
            load_merged_ali2(&index_files, &kb, ns.name, &tx)
        };

        if lookup.is_empty() && !ns.index_dir_rel.is_empty() {
            let _ = tx.send(PipelineEvent::Error(format!(
                "[!] {} — empty index after merge, skipping",
                ns.name
            )));
            continue;
        }

        let ns_decrypted_before = *decrypted.lock().unwrap();
        let ns_errors_before    = *errors.lock().unwrap();

        let tx_par = tx.clone();
        stems.par_iter().for_each_with(tx_par, |tx, stem| {
            let blob_rel = if ns.is_flat {
                format!("{}/{}", ns.blob_dir_rel, stem)
            } else {
                let prefix = if stem.len() >= 2 { &stem[..2] } else { stem.as_str() };
                format!("{}/{}/{}.aladin", ns.blob_dir_rel, prefix, stem)
            };
            let blob_path = pull_dir.join(&blob_rel);

            match decrypt_one_blob(&blob_path, stem, &ns.key, &kb, &lookup) {
                Ok((plaintext, is_plain, sig_name)) => {
                    let out_path = decrypt_dir.join(&blob_rel);
                    match write_decrypted(&out_path, &plaintext) {
                        Ok(()) => {
                            if is_plain {
                                let _ = tx.send(PipelineEvent::Log(format!(
                                    "[·] {}/{stem}: keyIdHash is 0, treated as plain text",
                                    ns.name
                                )));
                            }
                            if sig_name.is_none() && ns.name != "aladin" && ns.name != "Data" {
                                *errors.lock().unwrap() += 1;
                                let _ = tx.send(PipelineEvent::Error(format!(
                                    "[!] {}/{stem}: not a recognized Unity file (unknown signature)",
                                    ns.name
                                )));
                            }

                            state.lock().unwrap().mark_processed(ns.name, stem);
                            let current = {
                                let mut d = decrypted.lock().unwrap();
                                *d += 1;
                                *d
                            };
                            let _ = tx.send(PipelineEvent::Progress { current, total });
                        }
                        Err(e) => {
                            *errors.lock().unwrap() += 1;
                            let _ = tx.send(PipelineEvent::Error(
                                format!("[!] Write {}/{stem}: {e}", ns.name),
                            ));
                        }
                    }
                }
                Err(e) => {
                    *errors.lock().unwrap() += 1;
                    let _ = tx.send(PipelineEvent::Error(format!("[!] {}/{stem}: {e}", ns.name)));
                }
            }
        });

        let ns_decrypted = *decrypted.lock().unwrap() - ns_decrypted_before;
        let ns_errors    = *errors.lock().unwrap()    - ns_errors_before;
        let _ = tx.send(PipelineEvent::Log(format!(
            "[✓] {} — {ns_decrypted} decrypted, {ns_errors} errors",
            ns.name,
        )));
    }

    // ── 4. Save state.json once ──────────────────────────────────────────────
    let st = state.lock().unwrap();
    let _ = st.save(output_dir);
    drop(st);

    let d = *decrypted.lock().unwrap();
    let e = *errors.lock().unwrap();
    let _ = tx.send(PipelineEvent::Done { decrypted: d, errors: e });
}

// ── Key file discovery ───────────────────────────────────────────────────────

/// Finds the key file in `DefaultMasterData/blob/*/*.aladin`.
/// Criterion: exactly 32 bytes, filename = hex u64.
fn find_key_blob(pull_dir: &Path) -> Result<(std::path::PathBuf, u64), String> {
    let key_dir = pull_dir.join(KEY_DIR_REL);
    let subdirs = std::fs::read_dir(&key_dir)
        .map_err(|e| format!("key directory inaccessible ({}): {e}", key_dir.display()))?;

    for subdir in subdirs.flatten() {
        if !subdir.path().is_dir() { continue; }
        let Ok(files) = std::fs::read_dir(subdir.path()) else { continue };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().map(|e| e == "aladin").unwrap_or(false)
                && std::fs::metadata(&path).map(|m| m.len() == 32).unwrap_or(false)
            {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(hash) = u64::from_str_radix(stem, 16) {
                        return Ok((path, hash));
                    }
                }
            }
        }
    }

    Err(format!("no key file (32 bytes) found in {}", key_dir.display()))
}

fn decrypt_main_key(path: &std::path::Path, key_id_hash: u64) -> Result<[u8; 32], String> {
    let data = std::fs::read(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    if data.len() != 32 {
        return Err(format!(
            "invalid size: {} bytes (expected 32)",
            data.len()
        ));
    }
    let enc: [u8; 32] = data.try_into().unwrap();
    Ok(decrypt_key_file(&enc, &GLOBAL_KEY, key_id_hash))
}

// ── Index discovery and loading ──────────────────────────────────────────────

/// Returns all `*.aladin` files in `<index_dir_rel>/*/<hash>.aladin`.
fn find_all_index_files(pull_dir: &Path, index_dir_rel: &str) -> Vec<(std::path::PathBuf, u64)> {
    let mut result = Vec::new();
    if index_dir_rel.is_empty() {
        return result;
    }
    let index_dir = pull_dir.join(index_dir_rel);
    let Ok(subdirs) = std::fs::read_dir(&index_dir) else { return result };
    for subdir in subdirs.flatten() {
        if !subdir.path().is_dir() { continue; }
        let Ok(files) = std::fs::read_dir(subdir.path()) else { continue };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().map(|e| e == "aladin").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(hash) = u64::from_str_radix(stem, 16) {
                        result.push((path, hash));
                    }
                }
            }
        }
    }
    result
}

/// Loads an ALI2 index file and returns the lookup.
///
/// Strategy: first attempts direct read (cleartext ALI2);
/// if the magic is absent, decrypts with `key_body` using
/// `stem_hash` as `content_hash` (bootstrap without circular dependency).
fn load_one_ali2_index(
    path: &std::path::Path,
    stem_hash: u64,
    key_body: &[u8; 32],
) -> Result<HashMap<u64, Ali2Entry>, String> {
    let raw = std::fs::read(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;

    // Attempt 1: cleartext ALI2
    if let Ok(map) = build_ali2_lookup(&raw) {
        return Ok(map);
    }

    // Attempt 2: ChaCha20 decrypt (stem_hash = content_hash of the index)
    let decrypted = decrypt_blob(&raw, key_body, stem_hash);
    build_ali2_lookup(&decrypted)
        .map_err(|e| format!("parse ALI2 (after decryption) {}: {e}", path.display()))
}

/// Loads all index files for a namespace and merges their entries.
fn load_merged_ali2(
    index_files: &[(std::path::PathBuf, u64)],
    key_body: &[u8; 32],
    ns_name: &str,
    tx: &EventSender,
) -> HashMap<u64, Ali2Entry> {
    let mut merged = HashMap::new();
    for (path, stem_hash) in index_files {
        match load_one_ali2_index(path, *stem_hash, key_body) {
            Ok(map) => {
                let n = map.len();
                merged.extend(map);
                let _ = tx.send(PipelineEvent::Log(format!(
                    "[✓] {ns_name} — index {:016x}, {n} entries",
                    stem_hash
                )));
            }
            Err(e) => {
                let _ = tx.send(PipelineEvent::Error(format!("[!] {ns_name} — {e}")));
            }
        }
    }
    merged
}

// ── Blob decryption ──────────────────────────────────────────────────────────

fn decrypt_one_blob(
    blob_path: &Path,
    stem: &str,
    kb_source: &KeySource,
    key_body: &[u8; 32],
    lookup: &HashMap<u64, Ali2Entry>,
) -> Result<(Vec<u8>, bool, Option<&'static str>), String> {
    let ciphertext = std::fs::read(blob_path)
        .map_err(|e| format!("read {}: {e}", blob_path.display()))?;

    if matches!(kb_source, KeySource::Plain) {
        let sig_name = get_unity_signature(&ciphertext);
        return Ok((ciphertext, true, sig_name));
    }

    let ck_hash = u64::from_str_radix(stem, 16)
        .map_err(|_| format!("invalid stem: {stem}"))?;

    let entry = lookup
        .get(&ck_hash)
        .ok_or_else(|| "entry not found in index".to_string())?;

    let is_plain = entry.key_id_hash == 0;
    let plaintext = if is_plain {
        ciphertext
    } else {
        decrypt_blob(&ciphertext, key_body, entry.content_hash)
    };

    let sig_name = get_unity_signature(&plaintext);

    Ok((plaintext, is_plain, sig_name))
}

const UNITY_SIGNATURES: &[(&[u8], &'static str)] = &[
    (b"UnityFS",                  "Unity AssetBundle (UnityFS)"),
    (b"AFS2",                     "Unity Audio (CRI AFS2/ACB)"),
    (b"@UTF",                     "Unity Audio (CRI UTF Table/AWB)"),
    (b"CRID",                     "Unity Video (CRI USM)"),
    (b"\x18\x00\x00\x00ABDL",     "Unity AssetBundle (ABDL legacy)"),
    (b"\x1c\x00\x00\x00ABDL",     "Unity AssetBundle (ABDL legacy)"),
    (b"\x14\x00\x00\x00ORTM",     "Unity AssetBundle (ORTM/LZMA legacy)"),
    (b"\x18\x00\x00\x00ORTM",     "Unity AssetBundle (ORTM/LZMA legacy)"),
];

fn get_unity_signature(data: &[u8]) -> Option<&'static str> {
    for (sig, name) in UNITY_SIGNATURES {
        if data.starts_with(sig) {
            return Some(name);
        }
    }
    None
}

fn write_decrypted(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, data)
}
