use std::time::Duration;
use serde::Deserialize;
use serde_json::Value;

/// Mirror of Weir's `/events` response envelope. Events are kept as opaque
/// JSON `Value`s — the agent forwards them verbatim and only ever reads the
/// numeric `id` (to advance its cursor), so it stays decoupled from Weir's
/// `UsageEvent` schema.
#[derive(Debug, Clone, Deserialize)]
pub struct EventsResponse {
    pub generation: String,
    pub events: Vec<Value>,
}

pub struct WeirClient {
    http: reqwest::Client,
    events_url: String,
}

impl WeirClient {
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
            .map_err(|e| format!("weir request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("weir returned status {}", resp.status()));
        }
        resp.json::<EventsResponse>()
            .await
            .map_err(|e| format!("weir response parse failed: {e}"))
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

        let client = WeirClient::new(format!("{}/events", mock.uri()), Duration::from_secs(5));
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
        let client = WeirClient::new(format!("{}/events", mock.uri()), Duration::from_secs(5));
        assert!(client.fetch(0, 500).await.is_err());
    }
}
