# weir-agent

A small Rust binary that polls a local [Weir](https://github.com/SymFynity/weir-proxy)
instance and forwards per-request usage metadata to a backend. One agent per Weir
instance.

Weir enforces budgets and policy on a single instance and keeps its event log in
memory. weir-agent is what gets that data off the box — to
[SymFynity](https://symfynity.com), or to any endpoint you point it at.

## What it sends — and what it never sends

weir-agent forwards **metadata only**: tenant, provider, model, tool *names*,
token counts, outcome, rule, and timestamp.

It never sends prompt content, response content, or tool call arguments. Those
never leave your Weir process, because Weir's `/events` endpoint does not expose
them in the first place — there is nothing here to opt out of.

The source is published so that claim is checkable rather than promised. It's
about 800 lines of Rust; read it, build it, run your own.

## Use it with your own backend

Nothing about the agent is SymFynity-specific. It POSTs a documented JSON
contract (see [Ingestion contract](#ingestion-contract)) to whatever
`WEIR_AGENT_BACKEND_URL` names. Point it at your own collector and it works the
same way.

The agent is deliberately simple: no alerting, no aggregation, no policy logic.
It polls, forwards, and tracks a cursor. Everything else is the backend's job.

## Quick start

Requires a recent stable Rust toolchain, and a Weir instance to poll.

```bash
git clone https://github.com/SymFynity/weir-agent
cd weir-agent
cargo build --release

cp weir-agent.example.env .env   # set BACKEND_URL, ORG_KEY, INSTANCE_ID
set -a && source .env && set +a
RUST_LOG=info ./target/release/weir-agent
```

The agent logs startup config and cycle outcomes to stderr, and shuts down
gracefully on Ctrl+C or SIGTERM.

## Configuration

All configuration is via environment variables. See `weir-agent.example.env`
for a template.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `WEIR_AGENT_BACKEND_URL` | Yes | — | Ingestion endpoint URL |
| `WEIR_AGENT_ORG_KEY` | Yes | — | Bearer token for backend authentication |
| `WEIR_AGENT_INSTANCE_ID` | Yes | — | Identifier for this Weir instance (e.g. `prod-us-east`) |
| `WEIR_AGENT_EVENTS_URL` | No | `http://localhost:8080/events` | Weir `/events` endpoint URL |
| `WEIR_AGENT_POLL_INTERVAL_SECS` | No | `15` | Seconds to sleep between poll cycles when idle |
| `WEIR_AGENT_BATCH_SIZE` | No | `500` | Max events per backend POST |
| `WEIR_AGENT_STATE_FILE` | No | `./weir-agent-state.json` | Path to persist cursor & generation |

Missing required variables cause a fatal startup error with a clear message.

The directory containing `WEIR_AGENT_STATE_FILE` must already exist — the agent
does not create it — and persist failures are logged as warnings rather than
being fatal, so run with logging enabled to notice them.

## How it works

### Poll cycle

1. **Fetch** — `GET /events?since=<cursor>&limit=<batch_size>` from Weir.
2. **Restart detection** — if the response `generation` differs from the
   persisted one, Weir has restarted and its event IDs have reset. Reset the
   cursor to 0 and refetch from the beginning of the new process.
3. **Ingest** — POST the batch to the backend with the instance ID, generation,
   and events.
4. **Advance & persist** — on a 2xx response, update the cursor to the highest
   event ID in the batch and atomically persist `(generation, cursor)`. On
   failure, hold the cursor and retry next cycle.

### At-least-once delivery

The agent guarantees at-least-once delivery of each event:

- The cursor only advances after a successful (2xx) backend POST.
- If the agent crashes after a successful ingest but before persisting the
  cursor, the batch is resent on restart.
- **The backend must deduplicate on `(instance_id, generation, event.id)`** to
  achieve exactly-once semantics.
- Events are lost only if the agent is offline long enough that they age out of
  Weir's bounded in-memory ring buffer.

### Backoff & drain

If a batch comes back full, the agent cycles immediately to check for more
(drain mode). If it's empty or short, the agent sleeps for the poll interval. On
transient failures — Weir or the backend unreachable — it backs off to the same
interval sleep.

## Ingestion contract

**Method & auth:**

- `POST <WEIR_AGENT_BACKEND_URL>`
- `Authorization: Bearer <WEIR_AGENT_ORG_KEY>`

**Body:**

```json
{
  "instance_id": "prod-us-east",
  "generation": "abc123",
  "events": [
    { "id": 1, "tenant": "acct_1", "provider": "openai", "model": "gpt-4o-mini",
      "tokens": { "prompt": 50, "completion": 25 }, "outcome": "completed" },
    { "id": 2, "tenant": "acct_2", "provider": "anthropic", "model": "claude-sonnet-5",
      "outcome": "policy_blocked", "rule": "blocked_tool:send_email" }
  ]
}
```

Events are the raw Weir `UsageEvent` JSON, forwarded verbatim. The backend must
treat any 2xx (200, 202, 204) as a successful ingest.

## State & cursor

State is persisted to a JSON file, updated atomically after each successful
ingest:

```json
{ "generation": "abc123", "cursor": 42 }
```

- **generation** — snapshot of Weir's process generation when the cursor was last
  updated. Used to detect restarts.
- **cursor** — the ID of the highest event successfully forwarded. The next poll
  requests events `since` this ID.

If the state file doesn't exist on startup, the agent begins from cursor 0 with
generation unknown.

## Development

```bash
cargo test
cargo build --release
```

## License

Business Source License 1.1 — see [`LICENSE`](LICENSE).

weir-agent is source-available, not open source. Running it in production inside
your own organisation is free and always will be — the Additional Use Grant
covers internal production use explicitly. You can read, modify, fork, and
self-host it, and point it at your own collector instead of ours. The one
prohibited use is offering it to third parties as a hosted or embedded service
competing with SymFynity's paid product.

Four years after any given version is published, that version becomes available
under the Apache License 2.0 automatically.

weir-agent 0.1.0 was published under Apache License 2.0 and remains available
under those terms. The Business Source License applies from 0.2.0 onward.
