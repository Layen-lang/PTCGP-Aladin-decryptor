// aladin-core/src/state.rs

use std::{collections::{HashMap, HashSet}, path::Path};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct ProcessingState {
    pub processed: HashMap<String, HashSet<String>>,
}

impl ProcessingState {
    /// Loads state.json from output_dir. Returns an empty state if absent or unparseable.
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

    pub fn is_processed(&self, namespace: &str, stem: &str) -> bool {
        self.processed
            .get(namespace)
            .map(|s| s.contains(stem))
            .unwrap_or(false)
    }

    pub fn mark_processed(&mut self, namespace: &str, stem: &str) {
        self.processed
            .entry(namespace.to_string())
            .or_default()
            .insert(stem.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state_is_empty() {
        let s = ProcessingState::default();
        assert!(!s.is_processed("Default", "abc123"));
    }

    #[test]
    fn test_mark_and_check() {
        let mut s = ProcessingState::default();
        s.mark_processed("Default", "0da9778d9b6bed0a");
        assert!(s.is_processed("Default", "0da9778d9b6bed0a"));
        assert!(!s.is_processed("Default", "cf461af74368f659"));
        assert!(!s.is_processed("aladin", "0da9778d9b6bed0a"));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ProcessingState::default();
        s.mark_processed("Default", "aabbccdd");
        s.mark_processed("Default", "11223344");
        s.mark_processed("aladin", "0da9778d9b6bed0a");
        s.save(dir.path()).unwrap();

        let loaded = ProcessingState::load(dir.path());
        assert!(loaded.is_processed("Default", "aabbccdd"));
        assert!(loaded.is_processed("Default", "11223344"));
        assert!(loaded.is_processed("aladin", "0da9778d9b6bed0a"));
        assert!(!loaded.is_processed("Default", "ffffffff"));
        assert!(!loaded.is_processed("aladin", "aabbccdd"));
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let s = ProcessingState::load(dir.path());
        assert!(!s.is_processed("Default", "anything"));
    }
}
