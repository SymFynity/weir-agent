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
