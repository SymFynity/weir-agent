use std::time::Duration;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use symfynity_agent::backend::BackendClient;
use symfynity_agent::forwarder::{CycleOutcome, Forwarder};
use symfynity_agent::state::AgentState;
use symfynity_agent::symfynity::SymfynityClient;

// A full drain cycle: SymFynity returns a full batch then a partial, the agent
// forwards both, advances the cursor, and persists it — verified against a
// real (mock) backend and a real state file on disk.
#[tokio::test]
async fn drains_backlog_across_cycles_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("state.json");

    let symfynity = MockServer::start().await;
    // since=0 -> two events (full batch of 2, so `more`)
    Mock::given(method("GET")).and(path("/events"))
        .and(wiremock::matchers::query_param("since", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "generation": "gen-1",
            "events": [ {"id": 1, "tenant": "a", "outcome": "completed"},
                        {"id": 2, "tenant": "b", "outcome": "budget_blocked"} ]
        })))
        .mount(&symfynity).await;
    // since=2 -> one event (partial, so drain ends)
    Mock::given(method("GET")).and(path("/events"))
        .and(wiremock::matchers::query_param("since", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "generation": "gen-1",
            "events": [ {"id": 3, "tenant": "c", "outcome": "completed"} ]
        })))
        .mount(&symfynity).await;

    let backend = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/ingest"))
        .respond_with(ResponseTemplate::new(202))
        .mount(&backend).await;

    let f = Forwarder {
        symfynity: SymfynityClient::new(format!("{}/events", symfynity.uri()), Duration::from_secs(5)),
        backend: BackendClient::new(format!("{}/v1/ingest", backend.uri()), "sk".into(), Duration::from_secs(5)),
        instance_id: "inst".into(),
        batch_size: 2,
        state_file: state_file.clone(),
    };
    let mut state = AgentState::default();

    // First cycle: starting from a fresh (default, empty-generation) state,
    // the agent adopts generation "gen-1" (its restart/bootstrap branch),
    // fetches from since=0, and forwards ids 1,2 — a full batch, so `more`.
    assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Forwarded { count: 2, more: true });
    assert_eq!(state.cursor, 2);
    // Second cycle: forwards id 3 (partial -> done)
    assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Forwarded { count: 1, more: false });
    assert_eq!(state.cursor, 3);

    // Cursor persisted across a simulated restart (reload from disk).
    assert_eq!(AgentState::load(&state_file), AgentState { generation: "gen-1".into(), cursor: 3 });
}
