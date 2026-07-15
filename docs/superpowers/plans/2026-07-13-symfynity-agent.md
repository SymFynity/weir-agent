# symfynity-agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build symfynity-agent — a small closed-source Rust binary that polls a local SymFynity `/events` endpoint and forwards per-request usage metadata to a hosted backend, with a disk-persisted `(generation, cursor)`, restart detection, at-least-once delivery, and robust failure handling.

**Architecture:** One async poll loop (Tokio + reqwest). Each cycle: GET `{symfynity}/events?since={cursor}&limit={batch}` → if the response `generation` differs from persisted, reset the cursor (SymFynity restarted) → POST `{instance_id, generation, events}` to the backend with a bearer org key → on 2xx, advance the cursor to the batch's max event `id` and persist it atomically. Any failure logs and holds the cursor for the next interval. Events are forwarded **verbatim** as opaque JSON (the agent reads only each event's `id` to advance the cursor; it never inspects or reshapes payloads).

**Tech Stack:** Rust, Tokio, reqwest (JSON + rustls), serde/serde_json, tracing. Dev: wiremock, tempfile.

## Global Constraints

- **Privacy line:** the agent forwards only what SymFynity's `/events` exposes (metadata), verbatim. It must not inspect, log, or persist event contents beyond each event's numeric `id` (needed for the cursor). Never log full event bodies at info level.
- **Deliberately dumb:** no alerting, aggregation, dedup, or policy logic in the agent. It forwards facts; the backend interprets them.
- **Robustness:** every network call has a timeout; no failure path panics or blocks the loop forever. Missing required config is a fatal startup error.
- **Delivery = at-least-once:** the cursor advances only after the backend acks. The backend dedupes on `(instance_id, generation, event.id)` — the agent forwards `generation` so that key is well-defined across SymFynity restarts (event `id` resets per generation).
- **Ingestion contract (fixed):** `POST {backend_url}`, header `Authorization: Bearer <org_key>`, body `{ "instance_id": String, "generation": String, "events": [<verbatim SymFynity UsageEvent JSON>] }`, any `2xx` = accepted.
- No secrets committed. `symfynity-agent-state.json` and `.env` are gitignored.

---

## File Structure

```
symfynity-agent/
├── Cargo.toml
├── .gitignore
├── README.md
├── symfynity-agent.example.env
├── src/
│   ├── main.rs        (wiring: load config, build clients, run loop w/ graceful shutdown)
│   ├── lib.rs         (module declarations; re-exports for tests)
│   ├── config.rs      (Config + from_env, fail-fast)
│   ├── state.rs       (AgentState { generation, cursor }, atomic save/load)
│   ├── symfynity.rs        (SymfynityClient: GET /events -> EventsResponse { generation, events: Vec<Value> })
│   ├── backend.rs     (BackendClient: POST ingest body, bearer auth)
│   └── forwarder.rs   (poll cycle: fetch -> restart-detect -> forward -> advance+persist; drain)
└── tests/
    └── forwarder_test.rs  (end-to-end against wiremock SymFynity + backend stubs)
```

---

### Task 1: Scaffold the Cargo project

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `.gitignore`
- Test: a trivial lib test to confirm the build

**Interfaces:**
- Produces: a compiling binary + lib crate named `symfynity_agent`.

- [ ] **Step 1: Verify `.gitignore`**

A `.gitignore` already exists at the repo root (committed alongside this plan). Do NOT overwrite it — it contains `!docs/` / `!docs/superpowers/` overrides that keep this plan tracked under the user's global gitignore. Just confirm it contains these entries (add any that are missing, keeping the `!docs/` lines):

```
!docs/
!docs/superpowers/
/target/
/.worktrees/
symfynity-agent-state.json
*.env
!symfynity-agent.example.env
```

- [ ] **Step 2: Create `Cargo.toml`**

```toml
[package]
name = "symfynity-agent"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "symfynity-agent"
path = "src/main.rs"

[lib]
name = "symfynity_agent"
path = "src/lib.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
wiremock = "0.6"
tempfile = "3"
```

- [ ] **Step 3: Create `src/lib.rs`**

```rust
pub mod backend;
pub mod config;
pub mod forwarder;
pub mod state;
pub mod symfynity;
```

(The modules are created in later tasks; for THIS task, create empty placeholder files so the crate compiles — `src/config.rs`, `src/state.rs`, `src/symfynity.rs`, `src/backend.rs`, `src/forwarder.rs` each containing only a `// placeholder` line is fine, OR declare the modules incrementally. To keep Task 1 self-contained and compiling, create `src/lib.rs` with NO module declarations yet, and add each `pub mod` line in the task that creates that module.)

Use this for Task 1's `src/lib.rs`:
```rust
// Module declarations are added by later tasks as each module is created.
```

- [ ] **Step 4: Create `src/main.rs`**

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("symfynity-agent starting");
    // Wiring is completed in a later task.
}
```

- [ ] **Step 5: Add a trivial lib test**

Append to `src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 6: Build and test**

Run: `cargo build`
Expected: 0 errors.
Run: `cargo test`
Expected: PASS (1 test).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore src/main.rs src/lib.rs
git commit -m "chore: scaffold symfynity-agent Cargo project"
```

---

### Task 2: Config

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs` (add `pub mod config;`)

**Interfaces:**
- Produces: `Config { events_url: String, backend_url: String, org_key: String, instance_id: String, poll_interval: Duration, batch_size: usize, state_file: PathBuf }`, `Config::from_env() -> Result<Config, String>` (Err with a clear message naming the missing/invalid var). Used by `main` (Task 7) and tests.

- [ ] **Step 1: Write the failing tests**

Create `src/config.rs`:
```rust
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub events_url: String,
    pub backend_url: String,
    pub org_key: String,
    pub instance_id: String,
    pub poll_interval: Duration,
    pub batch_size: usize,
    pub state_file: PathBuf,
}

impl Config {
    /// Reads config from environment variables. Required: SYMFYNITY_AGENT_BACKEND_URL,
    /// SYMFYNITY_AGENT_ORG_KEY, SYMFYNITY_AGENT_INSTANCE_ID. Others have defaults.
    pub fn from_env() -> Result<Self, String> {
        Self::from_source(|k| std::env::var(k).ok())
    }

    /// Testable core: `get` returns the value for a var name, if set.
    pub fn from_source(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let required = |key: &str, get: &dyn Fn(&str) -> Option<String>| {
            get(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("missing required config: {key}"))
        };
        let backend_url = required("SYMFYNITY_AGENT_BACKEND_URL", &get)?;
        let org_key = required("SYMFYNITY_AGENT_ORG_KEY", &get)?;
        let instance_id = required("SYMFYNITY_AGENT_INSTANCE_ID", &get)?;

        let events_url = get("SYMFYNITY_AGENT_EVENTS_URL")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "http://localhost:8080/events".to_string());

        let poll_interval_secs = match get("SYMFYNITY_AGENT_POLL_INTERVAL_SECS") {
            Some(v) => v.parse::<u64>().map_err(|_| {
                "SYMFYNITY_AGENT_POLL_INTERVAL_SECS must be a positive integer".to_string()
            })?,
            None => 15,
        };
        let batch_size = match get("SYMFYNITY_AGENT_BATCH_SIZE") {
            Some(v) => v
                .parse::<usize>()
                .map_err(|_| "SYMFYNITY_AGENT_BATCH_SIZE must be a positive integer".to_string())?,
            None => 500,
        };
        if batch_size == 0 {
            return Err("SYMFYNITY_AGENT_BATCH_SIZE must be greater than 0".to_string());
        }

        let state_file = get("SYMFYNITY_AGENT_STATE_FILE")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "./symfynity-agent-state.json".to_string())
            .into();

        Ok(Config {
            events_url,
            backend_url,
            org_key,
            instance_id,
            poll_interval: Duration::from_secs(poll_interval_secs),
            batch_size,
            state_file,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn source(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn parses_required_and_defaults() {
        let cfg = Config::from_source(source(&[
            ("SYMFYNITY_AGENT_BACKEND_URL", "https://backend.example/v1/ingest"),
            ("SYMFYNITY_AGENT_ORG_KEY", "sk-org-123"),
            ("SYMFYNITY_AGENT_INSTANCE_ID", "prod-us-east"),
        ]))
        .unwrap();
        assert_eq!(cfg.backend_url, "https://backend.example/v1/ingest");
        assert_eq!(cfg.org_key, "sk-org-123");
        assert_eq!(cfg.instance_id, "prod-us-east");
        assert_eq!(cfg.events_url, "http://localhost:8080/events");
        assert_eq!(cfg.poll_interval, Duration::from_secs(15));
        assert_eq!(cfg.batch_size, 500);
        assert_eq!(cfg.state_file, PathBuf::from("./symfynity-agent-state.json"));
    }

    #[test]
    fn missing_required_is_error() {
        let err = Config::from_source(source(&[("SYMFYNITY_AGENT_ORG_KEY", "x")])).unwrap_err();
        assert!(err.contains("SYMFYNITY_AGENT_BACKEND_URL"));
    }

    #[test]
    fn overrides_are_applied() {
        let cfg = Config::from_source(source(&[
            ("SYMFYNITY_AGENT_BACKEND_URL", "https://b/i"),
            ("SYMFYNITY_AGENT_ORG_KEY", "k"),
            ("SYMFYNITY_AGENT_INSTANCE_ID", "i"),
            ("SYMFYNITY_AGENT_EVENTS_URL", "http://symfynity:9000/events"),
            ("SYMFYNITY_AGENT_POLL_INTERVAL_SECS", "5"),
            ("SYMFYNITY_AGENT_BATCH_SIZE", "100"),
            ("SYMFYNITY_AGENT_STATE_FILE", "/var/lib/symfynity-agent/state.json"),
        ]))
        .unwrap();
        assert_eq!(cfg.events_url, "http://symfynity:9000/events");
        assert_eq!(cfg.poll_interval, Duration::from_secs(5));
        assert_eq!(cfg.batch_size, 100);
        assert_eq!(cfg.state_file, PathBuf::from("/var/lib/symfynity-agent/state.json"));
    }

    #[test]
    fn zero_batch_size_is_error() {
        let err = Config::from_source(source(&[
            ("SYMFYNITY_AGENT_BACKEND_URL", "https://b/i"),
            ("SYMFYNITY_AGENT_ORG_KEY", "k"),
            ("SYMFYNITY_AGENT_INSTANCE_ID", "i"),
            ("SYMFYNITY_AGENT_BATCH_SIZE", "0"),
        ]))
        .unwrap_err();
        assert!(err.contains("BATCH_SIZE"));
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `src/lib.rs`: `pub mod config;`

- [ ] **Step 3: Run tests**

Run: `cargo test --lib config`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/lib.rs
git commit -m "feat: config from env with fail-fast on missing required vars"
```

---

### Task 3: State (atomic, restart-safe cursor)

**Files:**
- Create: `src/state.rs`
- Modify: `src/lib.rs` (add `pub mod state;`)

**Interfaces:**
- Produces: `AgentState { generation: String, cursor: u64 }` (serde, Default = `{ "", 0 }`), `AgentState::load(path: &Path) -> AgentState` (returns Default on absent/unreadable/corrupt — never errors), `AgentState::save(&self, path: &Path) -> std::io::Result<()>` (atomic: write temp + rename). Used by `forwarder` (Task 6).

- [ ] **Step 1: Write the failing tests**

Create `src/state.rs`:
```rust
use std::path::Path;
use serde::{Deserialize, Serialize};

/// Persisted agent progress. `cursor` is the id of the last event
/// successfully forwarded WITHIN `generation`; both reset together when
/// SymFynity restarts (a new generation), since SymFynity's event ids restart at 1
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
```

- [ ] **Step 2: Wire the module**

Add to `src/lib.rs`: `pub mod state;`

- [ ] **Step 3: Run tests**

Run: `cargo test --lib state`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add src/state.rs src/lib.rs
git commit -m "feat: atomic, restart-safe agent state persistence"
```

---

### Task 4: SymFynity client (fetch `/events`)

**Files:**
- Create: `src/symfynity.rs`
- Modify: `src/lib.rs` (add `pub mod symfynity;`)

**Interfaces:**
- Produces: `EventsResponse { generation: String, events: Vec<serde_json::Value> }` (Deserialize — mirrors SymFynity's `/events` envelope; events kept as opaque `Value` so the agent is decoupled from SymFynity's exact `UsageEvent` schema), `SymfynityClient::new(events_url: String, timeout: Duration) -> Self`, `async fn fetch(&self, since: u64, limit: usize) -> Result<EventsResponse, String>`. Used by `forwarder` (Task 6). Also a free helper `max_event_id(events: &[Value]) -> Option<u64>` that reads each event's numeric `"id"` and returns the max.

- [ ] **Step 1: Write the failing tests**

Create `src/symfynity.rs`:
```rust
use std::time::Duration;
use serde::Deserialize;
use serde_json::Value;

/// Mirror of SymFynity's `/events` response envelope. Events are kept as opaque
/// JSON `Value`s — the agent forwards them verbatim and only ever reads the
/// numeric `id` (to advance its cursor), so it stays decoupled from SymFynity's
/// `UsageEvent` schema.
#[derive(Debug, Clone, Deserialize)]
pub struct EventsResponse {
    pub generation: String,
    pub events: Vec<Value>,
}

pub struct SymfynityClient {
    http: reqwest::Client,
    events_url: String,
}

impl SymfynityClient {
    pub fn new(events_url: String, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client builds");
        Self { http, events_url }
    }

    /// GET {events_url}?since={since}&limit={limit}. Returns the parsed
    /// envelope, or an error string on transport/status/parse failure.
    pub async fn fetch(&self, since: u64, limit: usize) -> Result<EventsResponse, String> {
        let resp = self
            .http
            .get(&self.events_url)
            .query(&[("since", since.to_string()), ("limit", limit.to_string())])
            .send()
            .await
            .map_err(|e| format!("symfynity request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("symfynity returned status {}", resp.status()));
        }
        resp.json::<EventsResponse>()
            .await
            .map_err(|e| format!("symfynity response parse failed: {e}"))
    }
}

/// The highest numeric `id` among `events`, or `None` if empty / no numeric ids.
pub fn max_event_id(events: &[Value]) -> Option<u64> {
    events.iter().filter_map(|e| e.get("id").and_then(|v| v.as_u64())).max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn max_event_id_reads_ids() {
        let events = vec![json!({"id": 3}), json!({"id": 7}), json!({"id": 5})];
        assert_eq!(max_event_id(&events), Some(7));
        assert_eq!(max_event_id(&[]), None);
    }

    #[tokio::test]
    async fn fetch_parses_envelope_and_sends_query() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/events"))
            .and(query_param("since", "10"))
            .and(query_param("limit", "500"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "generation": "gen-abc",
                "events": [ {"id": 11, "tenant": "acct_1", "outcome": "completed"} ]
            })))
            .mount(&mock)
            .await;

        let client = SymfynityClient::new(format!("{}/events", mock.uri()), Duration::from_secs(5));
        let resp = client.fetch(10, 500).await.unwrap();
        assert_eq!(resp.generation, "gen-abc");
        assert_eq!(resp.events.len(), 1);
        assert_eq!(max_event_id(&resp.events), Some(11));
    }

    #[tokio::test]
    async fn fetch_errors_on_non_success_status() {
        let mock = MockServer::start().await;
        Mock::given(method("GET")).and(path("/events"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock)
            .await;
        let client = SymfynityClient::new(format!("{}/events", mock.uri()), Duration::from_secs(5));
        assert!(client.fetch(0, 500).await.is_err());
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `src/lib.rs`: `pub mod symfynity;`

- [ ] **Step 3: Run tests**

Run: `cargo test --lib symfynity`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add src/symfynity.rs src/lib.rs
git commit -m "feat: SymFynity /events client with opaque verbatim events"
```

---

### Task 5: Backend client (POST ingest)

**Files:**
- Create: `src/backend.rs`
- Modify: `src/lib.rs` (add `pub mod backend;`)

**Interfaces:**
- Produces: `BackendClient::new(backend_url: String, org_key: String, timeout: Duration) -> Self`, `async fn ingest(&self, instance_id: &str, generation: &str, events: &[serde_json::Value]) -> Result<(), String>` (Ok on any 2xx; Err otherwise, including the status). Used by `forwarder` (Task 6). The POST body is `{ instance_id, generation, events }` and the request carries `Authorization: Bearer {org_key}`.

- [ ] **Step 1: Write the failing tests**

Create `src/backend.rs`:
```rust
use std::time::Duration;
use serde::Serialize;
use serde_json::Value;

pub struct BackendClient {
    http: reqwest::Client,
    backend_url: String,
    org_key: String,
}

#[derive(Serialize)]
struct IngestBody<'a> {
    instance_id: &'a str,
    generation: &'a str,
    events: &'a [Value],
}

impl BackendClient {
    pub fn new(backend_url: String, org_key: String, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client builds");
        Self { http, backend_url, org_key }
    }

    /// POSTs `{ instance_id, generation, events }` with a bearer org key.
    /// Ok on any 2xx; Err (with the status/transport detail) otherwise.
    pub async fn ingest(
        &self,
        instance_id: &str,
        generation: &str,
        events: &[Value],
    ) -> Result<(), String> {
        let body = IngestBody { instance_id, generation, events };
        let resp = self
            .http
            .post(&self.backend_url)
            .bearer_auth(&self.org_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("backend request failed: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("backend returned status {}", resp.status()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn ingest_posts_contract_body_with_bearer() {
        let mock = MockServer::start().await;
        let events = vec![json!({"id": 1, "tenant": "acct_1", "outcome": "completed"})];
        Mock::given(method("POST"))
            .and(path("/v1/ingest"))
            .and(header("authorization", "Bearer sk-org-123"))
            .and(body_json(json!({
                "instance_id": "prod-us-east",
                "generation": "gen-1",
                "events": [ {"id": 1, "tenant": "acct_1", "outcome": "completed"} ]
            })))
            .respond_with(ResponseTemplate::new(202))
            .mount(&mock)
            .await;

        let client = BackendClient::new(
            format!("{}/v1/ingest", mock.uri()),
            "sk-org-123".into(),
            Duration::from_secs(5),
        );
        client.ingest("prod-us-east", "gen-1", &events).await.unwrap();
    }

    #[tokio::test]
    async fn ingest_errors_on_non_2xx() {
        let mock = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock)
            .await;
        let client = BackendClient::new(
            format!("{}/v1/ingest", mock.uri()),
            "bad".into(),
            Duration::from_secs(5),
        );
        assert!(client.ingest("i", "g", &[json!({"id":1})]).await.is_err());
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `src/lib.rs`: `pub mod backend;`

- [ ] **Step 3: Run tests**

Run: `cargo test --lib backend`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add src/backend.rs src/lib.rs
git commit -m "feat: backend ingest client (bearer auth, contract body)"
```

---

### Task 6: Forwarder (the poll cycle)

**Files:**
- Create: `src/forwarder.rs`
- Modify: `src/lib.rs` (add `pub mod forwarder;`)

**Interfaces:**
- Consumes: `SymfynityClient` (Task 4), `BackendClient` (Task 5), `AgentState` (Task 3), `max_event_id` (Task 4).
- Produces: `Forwarder { symfynity, backend, instance_id, batch_size, state_file }` and `async fn run_cycle(&self, state: &mut AgentState) -> CycleOutcome`, where `CycleOutcome` is an enum `{ Idle, Forwarded { count: usize, more: bool }, Failed }`. `run_cycle` does exactly one fetch→(restart-detect)→forward→advance+persist step and returns whether there may be more to drain. The `main` loop (Task 7) calls `run_cycle` repeatedly, draining while `more == true`, sleeping between idle/failed cycles. Kept as a pure-ish method (takes `&mut AgentState`, persists via `state_file`) so it's unit-testable against wiremock.

- [ ] **Step 1: Write the failing tests**

Create `src/forwarder.rs`:
```rust
use std::path::PathBuf;

use crate::backend::BackendClient;
use crate::state::AgentState;
use crate::symfynity::{max_event_id, SymfynityClient};

pub struct Forwarder {
    pub symfynity: SymfynityClient,
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
    /// A transient failure (SymFynity or backend). Cursor unchanged; retry later.
    Failed,
}

impl Forwarder {
    /// One poll cycle: fetch since the current cursor, detect a SymFynity restart
    /// (generation change) and reset the cursor if so, forward the batch,
    /// and on success advance + persist the cursor. Never panics; a failure
    /// returns `Failed` with the cursor untouched.
    pub async fn run_cycle(&self, state: &mut AgentState) -> CycleOutcome {
        let resp = match self.symfynity.fetch(state.cursor, self.batch_size).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("symfynity fetch failed: {e}");
                return CycleOutcome::Failed;
            }
        };

        // Restart detection: a changed generation means SymFynity restarted and
        // its event ids restarted at 1, so our cursor is meaningless. Reset
        // to 0 and adopt the new generation. (Old un-forwarded events are
        // already gone — SymFynity's buffer is in-memory.) Re-fetch from 0 so we
        // don't skip the new process's early events.
        if resp.generation != state.generation {
            tracing::info!(
                "symfynity generation changed ({} -> {}); resetting cursor",
                state.generation, resp.generation
            );
            state.generation = resp.generation.clone();
            state.cursor = 0;
            let refetched = match self.symfynity.fetch(0, self.batch_size).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("symfynity refetch after generation change failed: {e}");
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
        symfynity: MockServer,
        backend: MockServer,
    }

    async fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        Harness {
            _dir: dir,
            state_file,
            symfynity: MockServer::start().await,
            backend: MockServer::start().await,
        }
    }

    fn forwarder(h: &Harness, batch_size: usize) -> Forwarder {
        Forwarder {
            symfynity: SymfynityClient::new(format!("{}/events", h.symfynity.uri()), Duration::from_secs(5)),
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

    async fn symfynity_returns(h: &Harness, generation: &str, events: serde_json::Value) {
        Mock::given(method("GET")).and(path("/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "generation": generation, "events": events
            })))
            .mount(&h.symfynity).await;
    }

    async fn backend_accepts(h: &Harness) {
        Mock::given(method("POST")).and(path("/v1/ingest"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&h.backend).await;
    }

    #[tokio::test]
    async fn forwards_and_advances_cursor() {
        let h = harness().await;
        symfynity_returns(&h, "gen-1", json!([{"id": 5}, {"id": 6}])).await;
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
        symfynity_returns(&h, "gen-1", json!([])).await;
        let f = forwarder(&h, 500);
        let mut state = AgentState { generation: "gen-1".into(), cursor: 9 };
        assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Idle);
        assert_eq!(state.cursor, 9);
    }

    #[tokio::test]
    async fn full_batch_signals_more() {
        let h = harness().await;
        symfynity_returns(&h, "gen-1", json!([{"id": 1}, {"id": 2}])).await;
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
        symfynity_returns(&h, "gen-1", json!([{"id": 5}])).await;
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
        // SymFynity restarted: stored generation gen-OLD/cursor 100, but SymFynity now
        // reports gen-NEW with low ids. The agent must reset and forward from 0.
        let h = harness().await;
        // First (since=100) and refetch (since=0) both hit the same mock,
        // which always returns gen-NEW with a low id.
        symfynity_returns(&h, "gen-NEW", json!([{"id": 2}])).await;
        backend_accepts(&h).await;
        let f = forwarder(&h, 500);
        let mut state = AgentState { generation: "gen-OLD".into(), cursor: 100 };

        let outcome = f.run_cycle(&mut state).await;
        assert_eq!(outcome, CycleOutcome::Forwarded { count: 1, more: false });
        assert_eq!(state.generation, "gen-NEW");
        assert_eq!(state.cursor, 2);
    }
}
```

- [ ] **Step 2: Wire the module**

Add to `src/lib.rs`: `pub mod forwarder;`

- [ ] **Step 3: Run tests**

Run: `cargo test --lib forwarder`
Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```bash
git add src/forwarder.rs src/lib.rs
git commit -m "feat: forwarder poll cycle with restart detection and at-least-once forwarding"
```

---

### Task 7: main wiring + poll loop driver

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `Config` (Task 2), `AgentState` (Task 3), `SymfynityClient`/`BackendClient` (Tasks 4/5), `Forwarder`/`CycleOutcome` (Task 6).
- Produces: the runnable binary — loads config (fatal on error), builds the forwarder, loads state, runs the poll loop with drain + interval sleep, until Ctrl+C/SIGTERM.

- [ ] **Step 1: Replace `src/main.rs`**

```rust
use std::time::Duration;

use symfynity_agent::backend::BackendClient;
use symfynity_agent::config::Config;
use symfynity_agent::forwarder::{CycleOutcome, Forwarder};
use symfynity_agent::state::AgentState;
use symfynity_agent::symfynity::SymfynityClient;

// Per-request timeout for the SymFynity and backend HTTP calls.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("symfynity-agent config error: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(
        "symfynity-agent starting: events_url={}, backend_url={}, instance_id={}, interval={}s, batch={}",
        config.events_url, config.backend_url, config.instance_id,
        config.poll_interval.as_secs(), config.batch_size
    );

    let forwarder = Forwarder {
        symfynity: SymfynityClient::new(config.events_url.clone(), HTTP_TIMEOUT),
        backend: BackendClient::new(config.backend_url.clone(), config.org_key.clone(), HTTP_TIMEOUT),
        instance_id: config.instance_id.clone(),
        batch_size: config.batch_size,
        state_file: config.state_file.clone(),
    };
    let mut state = AgentState::load(&config.state_file);
    tracing::info!("loaded state: generation={:?}, cursor={}", state.generation, state.cursor);

    let poll = async {
        loop {
            // Drain: keep cycling immediately while a full batch signals a backlog.
            loop {
                match forwarder.run_cycle(&mut state).await {
                    CycleOutcome::Forwarded { count, more } => {
                        tracing::info!("forwarded {count} events (more={more})");
                        if !more {
                            break;
                        }
                    }
                    CycleOutcome::Idle => break,
                    CycleOutcome::Failed => break, // back off to the interval sleep
                }
            }
            tokio::time::sleep(config.poll_interval).await;
        }
    };

    tokio::select! {
        _ = poll => {}
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received, exiting");
        }
    }
}

/// Waits for Ctrl+C or SIGTERM so the agent exits promptly under a process
/// manager or in a container (PID 1 with no init).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
```

- [ ] **Step 2: Build and smoke-check**

Run: `cargo build`
Expected: 0 errors.

Run (should exit non-zero with a config error, proving fail-fast):
`cargo run 2>&1 | head -3` (no env set → prints "symfynity-agent config error: missing required config: SYMFYNITY_AGENT_BACKEND_URL" and exits 1). Confirm, then move on. Do NOT leave a process running.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire config, forwarder, and poll loop with graceful shutdown"
```

---

### Task 8: End-to-end integration test

**Files:**
- Create: `tests/forwarder_test.rs`

**Interfaces:**
- Consumes: the public crate API (`Forwarder`, `SymfynityClient`, `BackendClient`, `AgentState`, `CycleOutcome`).

- [ ] **Step 1: Write the test**

Create `tests/forwarder_test.rs`:
```rust
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

    // First cycle: forwards ids 1,2 (full batch -> more)
    assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Forwarded { count: 2, more: true });
    assert_eq!(state.cursor, 2);
    // Second cycle: forwards id 3 (partial -> done)
    assert_eq!(f.run_cycle(&mut state).await, CycleOutcome::Forwarded { count: 1, more: false });
    assert_eq!(state.cursor, 3);

    // Cursor persisted across a simulated restart (reload from disk).
    assert_eq!(AgentState::load(&state_file), AgentState { generation: "gen-1".into(), cursor: 3 });
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test forwarder_test`
Expected: PASS (1 test).

Run the full suite: `cargo test`
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add tests/forwarder_test.rs
git commit -m "test: end-to-end drain + persist against wiremock stubs"
```

---

### Task 9: README + example env

**Files:**
- Create: `README.md`, `symfynity-agent.example.env`

**Interfaces:** none — docs only.

- [ ] **Step 1: Create `symfynity-agent.example.env`**

```bash
# symfynity-agent configuration (copy to a private .env; NEVER commit real secrets)

# Required
SYMFYNITY_AGENT_BACKEND_URL=https://ingest.example.com/v1/ingest
SYMFYNITY_AGENT_ORG_KEY=replace-with-your-org-api-key
SYMFYNITY_AGENT_INSTANCE_ID=prod-us-east

# Optional (defaults shown)
SYMFYNITY_AGENT_EVENTS_URL=http://localhost:8080/events
SYMFYNITY_AGENT_POLL_INTERVAL_SECS=15
SYMFYNITY_AGENT_BATCH_SIZE=500
SYMFYNITY_AGENT_STATE_FILE=./symfynity-agent-state.json
```

- [ ] **Step 2: Create `README.md`**

Write a concise README covering: what symfynity-agent is (a closed-source companion that forwards SymFynity's `/events` usage metadata to the hosted backend; metadata only, never content or tool arguments); how it works (poll loop, `(generation, cursor)` state, restart detection, at-least-once with backend dedup on `(instance_id, generation, id)`); configuration (the env-var table from the plan / example env); running it (`cargo run` with the env vars set, one agent per SymFynity instance); and the ingestion contract it targets (`POST` with bearer key and `{instance_id, generation, events}` body). Keep it accurate to what was built — do not document features not in the code (no alerting, no Docker image yet).

- [ ] **Step 3: Commit**

```bash
git add README.md symfynity-agent.example.env
git commit -m "docs: README and example env for symfynity-agent"
```

---

## Self-Review Notes

- **Spec coverage:** config + fail-fast (Task 2), atomic restart-safe state (Task 3), SymFynity `/events` client with verbatim opaque events (Task 4), backend ingest client matching the contract (Task 5), poll cycle with restart detection + at-least-once + drain + failure handling (Task 6), main loop + graceful shutdown (Task 7), end-to-end wiremock test (Task 8), docs (Task 9). Packaging (Docker) and the backend itself are out of scope per the spec.
- **Type consistency:** `AgentState` (state.rs) is used by the forwarder and persisted; `EventsResponse`/`max_event_id` (symfynity.rs) feed the forwarder; `BackendClient::ingest` signature `(instance_id, generation, events)` matches the contract body and the forwarder's call. `CycleOutcome` defined once (forwarder.rs), consumed by main.
- **Privacy line:** events are `serde_json::Value` forwarded verbatim; only `id` is read (via `max_event_id`); no event body is logged. `.env` and state file are gitignored.
- **At-least-once correctness:** the cursor advances only after a 2xx from the backend; a persist failure after a successful ingest is logged and tolerated (backend dedupes). Generation change resets the cursor to 0 and re-fetches so new-process events aren't skipped.
