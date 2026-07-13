# weir-agent

A lightweight Rust binary that polls a local Weir proxy instance and forwards per-request usage metadata to a hosted backend. One agent per Weir instance.

## What It Does

weir-agent polls the Weir proxy's `/events` endpoint, collects batch usage events, and forwards them to a backend ingestion service. It forwards **metadata only** — tenant, provider, model, tool names, token counts, outcome, rule, and timestamp. It never includes prompt content, response content, or tool arguments.

The agent is deliberately simple with no built-in alerting, aggregation, or policy logic; the backend is responsible for deduplication and further processing.

## How It Works

### Poll Cycle

1. **Fetch**: GET `/events?since=<cursor>&limit=<batch_size>` from Weir.
2. **Restart Detection**: If the response `generation` differs from the persisted one, Weir has restarted (and its event IDs reset). Reset the cursor to 0 and refetch from the beginning of the new process.
3. **Ingest**: POST the batch to the backend with the instance ID, generation, and events.
4. **Advance & Persist**: On 2xx response, update the cursor to the highest event ID in the batch and atomically persist `(generation, cursor)`. On failure, hold the cursor and retry next cycle.

### At-Least-Once Delivery

The agent guarantees at-least-once delivery of each event to the backend:

- The cursor only advances after a successful (2xx) backend POST.
- If the agent crashes after a successful ingest but before persisting the cursor, the batch is resent on restart.
- The backend must deduplicate on `(instance_id, generation, event.id)` to achieve exactly-once semantics.
- Events are lost only if the agent is offline long enough that they age out of Weir's bounded in-memory ring buffer.

### Persistence

State (generation and cursor) is persisted to a JSON file (`./weir-agent-state.json` by default, configurable). The file is updated atomically after each successful backend ingest.

### Backoff & Drain

- If a batch is full, the agent cycles immediately to check for more events (drain mode).
- If the batch is empty or smaller than the batch size, the agent sleeps for the configured poll interval before the next cycle.
- On transient failures (Weir or backend unreachable), the agent backs off to the interval sleep.

## Configuration

All configuration is via environment variables. See `weir-agent.example.env` for a template.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `WEIR_AGENT_BACKEND_URL` | Yes | — | Ingestion endpoint URL (e.g., `https://ingest.example.com/v1/ingest`) |
| `WEIR_AGENT_ORG_KEY` | Yes | — | Bearer token for backend authentication (e.g., `sk-org-123`) |
| `WEIR_AGENT_INSTANCE_ID` | Yes | — | Identifier for this Weir instance (e.g., `prod-us-east`); included in each ingest |
| `WEIR_AGENT_EVENTS_URL` | No | `http://localhost:8080/events` | Weir `/events` endpoint URL |
| `WEIR_AGENT_POLL_INTERVAL_SECS` | No | `15` | Seconds to sleep between poll cycles when idle |
| `WEIR_AGENT_BATCH_SIZE` | No | `500` | Max events per backend POST |
| `WEIR_AGENT_STATE_FILE` | No | `./weir-agent-state.json` | Path to persist cursor & generation |

The directory containing `WEIR_AGENT_STATE_FILE` must already exist — the agent does not create it — and persist failures are logged only as warnings (not fatal), so run with logging enabled to notice them.

Missing required variables cause a fatal startup error with a clear message.

## Running

Set the required environment variables (see `weir-agent.example.env`), then:

```bash
# Development
cargo run

# Release build
cargo build --release
./target/release/weir-agent
```

The agent logs startup config and cycle outcomes to stderr (via `RUST_LOG` tracing levels). It gracefully shuts down on Ctrl+C or SIGTERM.

```bash
# Set log level
RUST_LOG=info cargo run

# Or with a binary
RUST_LOG=warn ./target/release/weir-agent
```

## Ingestion Contract

The agent POSTs to the backend URL with the following contract:

**Method & Auth:**
- `POST <WEIR_AGENT_BACKEND_URL>`
- Header: `Authorization: Bearer <WEIR_AGENT_ORG_KEY>`

**Body** (JSON):
```json
{
  "instance_id": "prod-us-east",
  "generation": "abc123",
  "events": [
    { "id": 1, "tenant": "acct_1", "provider": "openai", "model": "gpt-4", "tokens": { "prompt": 50, "completion": 25 }, "outcome": "completed", ... },
    { "id": 2, "tenant": "acct_2", "provider": "anthropic", "model": "claude-3", ... }
  ]
}
```

Events are the raw Weir `UsageEvent` JSON, forwarded verbatim. The backend must handle any 2xx (e.g., 200, 202, 204) as a successful ingest and deduplicate on `(instance_id, generation, event.id)` to ensure idempotency.

## State & Cursor

The agent persists a JSON state file containing:

```json
{
  "generation": "abc123",
  "cursor": 42
}
```

- **generation**: Snapshot of Weir's process generation when the cursor was last updated. Used to detect restarts.
- **cursor**: The ID of the highest event successfully forwarded. The next poll requests events `since` this ID.

If the state file doesn't exist on startup, the agent begins from cursor 0 with generation unknown.
