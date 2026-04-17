// aladin-core/src/state.rs

use std::{collections::HashSet, path::Path};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct ProcessingState {
    pub processed: HashSet<String>,
}

impl ProcessingState {
    /// Loads state.json from output_dir. Returns an empty state if absent.
    pub fn load(output_dir: &Path) -> Self {
        std::fs::read_to_string(output_dir.join("state.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Saves state.json to output_dir.
    pub fn save(&self, output_dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).expect("serialize state");
        std::fs::write(output_dir.join("state.json"), json)
    }

    pub fn is_processed(&self, stem: &str) -> bool {
        self.processed.contains(stem)
    }

    pub fn mark_processed(&mut self, stem: &str) {
        self.processed.insert(stem.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state_is_empty() {
        let s = ProcessingState::default();
        assert!(!s.is_processed("abc123"));
    }

    #[test]
    fn test_mark_and_check() {
        let mut s = ProcessingState::default();
        s.mark_processed("0da9778d9b6bed0a");
        assert!(s.is_processed("0da9778d9b6bed0a"));
        assert!(!s.is_processed("cf461af74368f659"));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ProcessingState::default();
        s.mark_processed("aabbccdd");
        s.mark_processed("11223344");
        s.save(dir.path()).unwrap();

        let loaded = ProcessingState::load(dir.path());
        assert!(loaded.is_processed("aabbccdd"));
        assert!(loaded.is_processed("11223344"));
        assert!(!loaded.is_processed("ffffffff"));
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let s = ProcessingState::load(dir.path());
        assert!(!s.is_processed("anything"));
    }
}
