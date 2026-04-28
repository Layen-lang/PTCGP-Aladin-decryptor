# PTCGP Aladin Decryptor

A desktop GUI tool to extract and decrypt game asset files (`.aladin`) from **Pokémon TCG Pocket** directly from an Android device via ADB.

> **Disclaimer:** This project is an independent research tool. It is not affiliated with, endorsed by, or connected to Nintendo, Creatures Inc., or DeNA. Use it only on files you own. Respect the game's Terms of Service.

---

## Prerequisites

| Requirement | Notes |
|---|---|
| [Rust](https://rustup.rs) ≥ 1.75 | Install via `rustup` |
| [ADB](https://developer.android.com/tools/adb) | Must be in your `PATH` |
| Android device | USB debugging enabled, Pokémon TCG Pocket installed |

---

## Build

```bash
git clone https://github.com/Layen-lang/PTCGP-Aladin-decryptor.git
cd PTCGP-Aladin-decryptor
cargo build --release
```

The binary is produced at `target/release/aladin-app`.

---

## Usage

1. **Connect your Android device** via USB with USB debugging enabled
2. **Run the application**
   ```bash
   ./target/release/aladin-app
   ```
3. **Refresh** the device list — your device should appear in the left panel
4. **Select your device**
5. **Choose an output directory** using the Browse button
6. **Pull** — fetches the encrypted files from the device (only new files on subsequent runs)
7. **Decrypt** — decrypts all pulled blobs and writes plaintext files to your output directory

Decrypted files are written to `<output_dir>/decrypted/Sharin.Resources/<namespace>/blob/`, where `<namespace>` is either `Default` or `aladin`. The tool keeps a `state.json` to track already-processed files (per namespace) and skip them on future runs.

---

## Using the decrypted files

The two namespaces hold different kinds of payloads and need different tooling:

| Namespace | Content | Tooling |
|---|---|---|
| `Default/blob/` | Unity asset bundles (UnityFS) — textures, sprites, audio, serialized data | AssetStudio (see below) |
| `aladin/blob/` | Master data tables (game logic, card definitions, etc.) — **not** Unity bundles | **In development** — no public extractor yet |

### Default — AssetStudioModCLI

Download [AssetStudio](https://github.com/Razviar/assetstudio) and use the CLI for batch export:

```
AssetStudioModCLI.exe <output_dir>/decrypted/Sharin.Resources/Default/blob -m export -o <export_dir> --unity-version 2022.3.58f1
```

The Unity version shown here was current at the time of writing — it may change with game updates. You can find the exact version by opening any decrypted blob in a hex editor and reading the null-terminated string at offset 12 (right after the `UnityFS` magic bytes and the 4-byte format version).

### aladin — master data (WIP)

The `aladin/` namespace decrypts to master-data files (not UnityFS). A dedicated parser is **still in development**; for now, the decrypted blobs in `decrypted/Sharin.Resources/aladin/blob/` are usable as-is for inspection (hex editor, custom scripts) but no batch-export tool is shipped yet.

---

## Project structure

```
aladin-core/   — decryption library (ACP cipher, ALI2 index parser, ADB wrapper, pipeline)
aladin-app/    — egui desktop GUI
```

---

## Legal notice

This software is provided for **personal research and educational purposes only**. Decrypting or redistributing game assets may violate the Pokémon TCG Pocket Terms of Service and applicable copyright law. The authors take no responsibility for any misuse.
