use std::path::Path;
use serde::{Deserialize, Serialize};

/// Persisted agent progress. `cursor` is the id of the last event
/// successfully forwarded WITHIN `generation`; both reset together when
/// Weir restarts (a new generation), since Weir's event ids restart at 1
/// each process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentState {
    pub generation: String,
    pub cursor: u64,
}

impl AgentState {
    /// Loads state from `path`. A missing, unreadable, or corrupt file
    /// yields `AgentState::default()` (forward from the start of whatever
    /// is currently buffered) rather than an error — the agent must start
    /// cleanly regardless of prior state.
    pub fn load(path: &Path) -> AgentState {
        match std::fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                tracing::warn!("ignoring unreadable state file {}: {e}", path.display());
                AgentState::default()
            }),
            Err(_) => AgentState::default(),
        }
    }

    /// Atomically persists state: write to a sibling temp file, then rename
    /// over the target, so a crash mid-write cannot corrupt the state file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string(self).expect("AgentState serializes");
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let s = AgentState { generation: "gen-1".into(), cursor: 42 };
        s.save(&path).unwrap();
        assert_eq!(AgentState::load(&path), s);
    }

    #[test]
    fn absent_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert_eq!(AgentState::load(&path), AgentState::default());
        assert_eq!(AgentState::load(&path).cursor, 0);
    }

    #[test]
    fn corrupt_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "not json {{{").unwrap();
        assert_eq!(AgentState::load(&path), AgentState::default());
    }

    #[test]
    fn save_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        AgentState { generation: "a".into(), cursor: 1 }.save(&path).unwrap();
        AgentState { generation: "b".into(), cursor: 9 }.save(&path).unwrap();
        assert_eq!(AgentState::load(&path), AgentState { generation: "b".into(), cursor: 9 });
    }
}
