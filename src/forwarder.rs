use std::path::PathBuf;

use crate::backend::BackendClient;
use crate::state::AgentState;
use crate::weir::{max_event_id, WeirClient};

pub struct Forwarder {
    pub weir: WeirClient,
    pub backend: BackendClient,
    pub instance_id: String,
    pub batch_size: usize,
    pub state_file: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CycleOutcome {
    /// Nothing to forward this cycle.
    Idle,
    /// Forwarded `count` events; `more` is true if the batch was full (a
    /// backlog may remain and the caller should cycle again immediately).
    Forwarded { count: usize, more: bool },
    /// A transient failure (Weir or backend). Cursor unchanged; retry later.
    Failed,
}

impl Forwarder {
    /// One poll cycle: fetch since the current cursor, detect a Weir restart
    /// (generation change) and reset the cursor if so, forward the batch,
    /// and on success advance + persist the cursor. Never panics; a failure
    /// returns `Failed` with the cursor untouched.
    pub async fn run_cycle(&self, state: &mut AgentState) -> CycleOutcome {
        let resp = match self.weir.fetch(state.cursor, self.batch_size).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("weir fetch failed: {e}");
                return CycleOutcome::Failed;
            }
        };

        // Restart detection: a changed generation means Weir restarted and
        // its event ids restarted at 1, so our cursor is meaningless. Reset
        // to 0 and adopt the new generation. (Old un-forwarded events are
        // already gone — Weir's buffer is in-memory.) Re-fetch from 0 so we
        // don't skip the new process's early events.
        if resp.generation != state.generation {
            tracing::info!(
                "weir generation changed ({} -> {}); resetting cursor",
                state.generation, resp.generation
            );
            state.generation = resp.generation.clone();
            state.cursor = 0;
            let refetched = match self.weir.fetch(0, self.batch_size).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("weir refetch after generation change failed: {e}");
                    // Persist the new generation + reset cursor so we don't
                    // repeatedly reset; retry the fetch next cycle.
                    let _ = state.save(&self.state_file);
                    return CycleOutcome::Failed;
                }
            };
            return self.forward(state, refetched.events).await;
        }

        self.forward(state, resp.events).await
    }

    async fn forward(&self, state: &mut AgentState, events: Vec<serde_json::Value>) -> CycleOutcome {
        if events.is_empty() {
            // Persist in case only the generation changed (cursor reset).
            let _ = state.save(&self.state_file);
            return CycleOutcome::Idle;
        }
        let count = events.len();
        let more = count >= self.batch_size;
        match self.backend.ingest(&self.instance_id, &state.generation, &events).await {
            Ok(()) => {
                if let Some(max_id) = max_event_id(&events) {
                    state.cursor = max_id;
                }
                if let Err(e) = state.save(&self.state_file) {
                    // Backend accepted but we failed to persist the cursor.
                    // Next run re-sends this batch; the backend dedupes on
                    // (instance_id, generation, id), so this is safe.
                    tracing::warn!("failed to persist cursor: {e}");
                }
                CycleOutcome::Forwarded { count, more }
            }
            Err(e) => {
                tracing::warn!("backend ingest failed: {e}");
                CycleOutcome::Failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Harness {
        _dir: tempfile::TempDir,
        state_file: PathBuf,
        weir: MockServer,
        backend: MockServer,
    }

    async fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        Harness {
            _dir: dir,
            state_file,
            weir: MockServer::start().await,
            backend: MockServer::start().await,
        }
    }

    fn forwarder(h: &Harness, batch_size: usize) -> Forwarder {
        Forwarder {
            weir: WeirClient::new(format!("{}/events", h.weir.uri()), Duration::from_secs(5)),
            backend: BackendClient::new(
                format!("{}/v1/ingest", h.backend.uri()),
                "sk".into(),
                Duration::from_secs(5),
            ),
            instance_id: "inst".into(),
            batch_size,
            state_file: h.state_file.clone(),
        }
    }

    async fn weir_returns(h: &Harness, generation: &str, events: serde_json::Value) {
        Mock::given(method("GET")).and(path("/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "generation": generation, "events": events
            })))
            .mount(&h.weir).await;
    }

    async fn backend_accepts(h: &Harness) {
        Mock::given(method("POST")).and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&h.backend).await;
    }

    #[tokio::test]
    async fn forwards_and_advances_cursor() {
        let h = harness().await;
        weir_returns(&h, "gen-1", json!([{"id": 5}, {"id": 6}])).await;
        backend_accepts(&h).await;
        let f = forwarder(&h, 500);
        let mut state = AgentState::default();

        let outcome = f.run_cycle(&mut state).await;
        assert_eq!(outcome, CycleOutcome::Forwarded { count: 2, more: false });
        assert_eq!(state.cursor, 6);
        assert_eq!(state.generation, "gen-1");
        // persisted
        assert_eq!(AgentState::load(&h.state_file).cursor, 6);
    }

    #[tokio::test]
    async fn empty_response_is_idle_and_holds_cursor() {
        let h = harness().await;
        weir_returns(&h, "gen-1", json!([])).await;
        let f = forwarder(&h, 500);
        let mut state = AgentState { generation: "gen-1".into(), cursor: 9 };
        assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Idle);
        assert_eq!(state.cursor, 9);
    }

    #[tokio::test]
    async fn full_batch_signals_more() {
        let h = harness().await;
        weir_returns(&h, "gen-1", json!([{"id": 1}, {"id": 2}])).await;
        backend_accepts(&h).await;
        let f = forwarder(&h, 2); // batch_size == returned count
        let mut state = AgentState { generation: "gen-1".into(), cursor: 0 };
        assert_eq!(
            f.run_cycle(&mut state).await,
            CycleOutcome::Forwarded { count: 2, more: true }
        );
    }

    #[tokio::test]
    async fn backend_failure_holds_cursor() {
        let h = harness().await;
        weir_returns(&h, "gen-1", json!([{"id": 5}])).await;
        Mock::given(method("POST")).and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&h.backend).await;
        let f = forwarder(&h, 500);
        let mut state = AgentState { generation: "gen-1".into(), cursor: 0 };
        assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Failed);
        assert_eq!(state.cursor, 0); // unchanged
    }

    #[tokio::test]
    async fn generation_change_resets_cursor() {
        // Weir restarted: stored generation gen-OLD/cursor 100, but Weir now
        // reports gen-NEW with low ids. The agent must reset and forward from 0.
        let h = harness().await;
        // First (since=100) and refetch (since=0) both hit the same mock,
        // which always returns gen-NEW with a low id.
        weir_returns(&h, "gen-NEW", json!([{"id": 2}])).await;
        backend_accepts(&h).await;
        let f = forwarder(&h, 500);
        let mut state = AgentState { generation: "gen-OLD".into(), cursor: 100 };

        let outcome = f.run_cycle(&mut state).await;
        assert_eq!(outcome, CycleOutcome::Forwarded { count: 1, more: false });
        assert_eq!(state.generation, "gen-NEW");
        assert_eq!(state.cursor, 2);
    }
}
