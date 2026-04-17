// aladin-core/src/adb.rs

use std::{
    path::Path,
    process::Command,
};

pub const REMOTE_BASE: &str =
    "/storage/emulated/0/Android/data/jp.pokemon.pokemontcgp/files";

#[derive(Debug, Clone)]
pub struct AdbDevice {
    pub serial: String,
    pub status: String,
}

/// Lists connected ADB devices, or returns an error if adb is not found in PATH.
/// Restarts the ADB server first to ensure a fresh device scan.
pub fn list_devices_result() -> Result<Vec<AdbDevice>, String> {
    Command::new("adb")
        .arg("kill-server")
        .output()
        .map_err(|_| "adb not found in PATH".to_string())?;
    Command::new("adb")
        .arg("start-server")
        .output()
        .map_err(|_| "adb not found in PATH".to_string())?;
    let output = Command::new("adb")
        .arg("devices")
        .output()
        .map_err(|_| "adb not found in PATH".to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .skip(1) // skip "List of devices attached"
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 && !parts[0].trim().is_empty() {
                Some(AdbDevice {
                    serial: parts[0].trim().to_string(),
                    status: parts[1].trim().to_string(),
                })
            } else {
                None
            }
        })
        .collect())
}

/// Lists connected ADB devices, silently returning an empty list on any error.
pub fn list_devices() -> Vec<AdbDevice> {
    list_devices_result().unwrap_or_default()
}

/// Lists all .aladin files on the device (full remote path).
pub fn list_remote_aladin_files(serial: &str) -> Result<Vec<String>, String> {
    let output = Command::new("adb")
        .args([
            "-s", serial,
            "shell",
            "find", REMOTE_BASE,
            "-name", "*.aladin",
            "-type", "f",
        ])
        .output()
        .map_err(|e| format!("adb shell find: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect())
}

/// Extracts the stem (name without extension) from a remote path.
/// E.g.: ".../blob/0d/0da9778d9b6bed0a.aladin" → "0da9778d9b6bed0a"
pub fn remote_path_to_stem(remote_path: &str) -> &str {
    let name = remote_path.rsplit('/').next().unwrap_or(remote_path);
    name.strip_suffix(".aladin").unwrap_or(name)
}

/// Extracts the relative path from REMOTE_BASE.
/// E.g.: ".../files/Sharin.Resources/Default/blob/0d/file.aladin"
///      → "Sharin.Resources/Default/blob/0d/file.aladin"
pub fn relative_remote_path(remote_path: &str) -> &str {
    remote_path
        .find(REMOTE_BASE)
        .map(|i| &remote_path[i + REMOTE_BASE.len()..])
        .unwrap_or(remote_path)
        .trim_start_matches('/')
}

const SDCARD_TMP: &str = "/sdcard/ptcgp_tmp";

/// Recursively pulls a remote directory.
/// Copies to /sdcard first to bypass Android 11+ restrictions on /Android/data/.
pub fn pull_directory(serial: &str, remote_path: &str, local_dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(local_dest)
        .map_err(|e| format!("mkdir {}: {e}", local_dest.display()))?;

    // 1. Copy to sdcard
    shell_cmd(serial, &["mkdir", "-p", SDCARD_TMP])?;
    shell_cmd(serial, &["cp", "-r", remote_path, SDCARD_TMP])?;

    // 2. Pull from sdcard
    let dirname = remote_path.rsplit('/').next().unwrap_or("files");
    let tmp_path = format!("{SDCARD_TMP}/{dirname}");
    let dest_str = local_dest
        .to_str()
        .ok_or_else(|| format!("non-UTF8 path: {}", local_dest.display()))?;
    let status = Command::new("adb")
        .args(["-s", serial, "pull", &tmp_path, dest_str])
        .status()
        .map_err(|e| format!("adb pull: {e}"))?;

    // 3. Cleanup
    let _ = shell_cmd(serial, &["rm", "-rf", SDCARD_TMP]);

    if status.success() {
        Ok(())
    } else {
        Err(format!("adb pull dir failed for {remote_path}"))
    }
}

/// Lists the stems of Default blobs on the device (without downloading).
/// Returns hex stems (filenames without `.aladin`).
pub fn list_remote_blob_stems(serial: &str) -> Result<Vec<String>, String> {
    let blob_path = format!("{}/Sharin.Resources/Default/blob", REMOTE_BASE);
    let output = Command::new("adb")
        .args([
            "-s", serial,
            "shell",
            "find", &blob_path,
            "-name", "*.aladin",
            "-type", "f",
        ])
        .output()
        .map_err(|e| format!("adb shell find: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let name = l.trim().rsplit('/').next()?;
            Some(name.strip_suffix(".aladin").unwrap_or(name).to_string())
        })
        .collect())
}

/// Pulls a single remote file to a local path (creates parent directories).
/// Copies to /sdcard first to bypass Android 11+ restrictions on /Android/data/.
pub fn pull_file(serial: &str, remote_path: &str, local_dest: &Path) -> Result<(), String> {
    if let Some(parent) = local_dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let filename = remote_path.rsplit('/').next().unwrap_or("file");
    let tmp_path = format!("{SDCARD_TMP}/{filename}");

    // 1. Copy to sdcard
    shell_cmd(serial, &["mkdir", "-p", SDCARD_TMP])?;
    shell_cmd(serial, &["cp", remote_path, &tmp_path])?;

    // 2. Pull from sdcard
    let dest_str = local_dest
        .to_str()
        .ok_or_else(|| format!("non-UTF8 path: {}", local_dest.display()))?;
    let status = Command::new("adb")
        .args(["-s", serial, "pull", &tmp_path, dest_str])
        .status()
        .map_err(|e| format!("adb pull: {e}"))?;

    // 3. Cleanup
    let _ = shell_cmd(serial, &["rm", "-f", &tmp_path]);

    if status.success() {
        Ok(())
    } else {
        Err(format!("adb pull failed for {remote_path}"))
    }
}

fn shell_cmd(serial: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("adb");
    cmd.args(["-s", serial, "shell"]);
    cmd.args(args);
    let status = cmd.status().map_err(|e| format!("adb shell: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("adb shell {} failed", args.join(" ")))
    }
}
