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
    /// Reads config from environment variables. Required: WEIR_AGENT_BACKEND_URL,
    /// WEIR_AGENT_ORG_KEY, WEIR_AGENT_INSTANCE_ID. Others have defaults.
    pub fn from_env() -> Result<Self, String> {
        Self::from_source(|k| std::env::var(k).ok())
    }

    /// Testable core: `get` returns the value for a var name, if set.
    pub fn from_source(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let required = |key: &str, get: &dyn Fn(&str) -> Option<String>| {
            get(key).filter(|v| !v.is_empty()).ok_or_else(|| format!("missing required config: {key}"))
        };
        let backend_url = required("WEIR_AGENT_BACKEND_URL", &get)?;
        let org_key = required("WEIR_AGENT_ORG_KEY", &get)?;
        let instance_id = required("WEIR_AGENT_INSTANCE_ID", &get)?;

        let events_url = get("WEIR_AGENT_EVENTS_URL")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "http://localhost:8080/events".to_string());

        let poll_interval_secs = match get("WEIR_AGENT_POLL_INTERVAL_SECS") {
            Some(v) => v.parse::<u64>().map_err(|_| {
                "WEIR_AGENT_POLL_INTERVAL_SECS must be a positive integer".to_string()
            })?,
            None => 15,
        };
        if poll_interval_secs == 0 {
            return Err("WEIR_AGENT_POLL_INTERVAL_SECS must be greater than 0".to_string());
        }
        let batch_size = match get("WEIR_AGENT_BATCH_SIZE") {
            Some(v) => v
                .parse::<usize>()
                .map_err(|_| "WEIR_AGENT_BATCH_SIZE must be a positive integer".to_string())?,
            None => 500,
        };
        if batch_size == 0 {
            return Err("WEIR_AGENT_BATCH_SIZE must be greater than 0".to_string());
        }

        let state_file = get("WEIR_AGENT_STATE_FILE")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "./weir-agent-state.json".to_string())
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
            ("WEIR_AGENT_BACKEND_URL", "https://backend.example/v1/ingest"),
            ("WEIR_AGENT_ORG_KEY", "sk-org-123"),
            ("WEIR_AGENT_INSTANCE_ID", "prod-us-east"),
        ]))
        .unwrap();
        assert_eq!(cfg.backend_url, "https://backend.example/v1/ingest");
        assert_eq!(cfg.org_key, "sk-org-123");
        assert_eq!(cfg.instance_id, "prod-us-east");
        assert_eq!(cfg.events_url, "http://localhost:8080/events");
        assert_eq!(cfg.poll_interval, Duration::from_secs(15));
        assert_eq!(cfg.batch_size, 500);
        assert_eq!(cfg.state_file, PathBuf::from("./weir-agent-state.json"));
    }

    #[test]
    fn missing_required_is_error() {
        let err = Config::from_source(source(&[("WEIR_AGENT_ORG_KEY", "x")])).unwrap_err();
        assert!(err.contains("WEIR_AGENT_BACKEND_URL"));
    }

    #[test]
    fn overrides_are_applied() {
        let cfg = Config::from_source(source(&[
            ("WEIR_AGENT_BACKEND_URL", "https://b/i"),
            ("WEIR_AGENT_ORG_KEY", "k"),
            ("WEIR_AGENT_INSTANCE_ID", "i"),
            ("WEIR_AGENT_EVENTS_URL", "http://weir:9000/events"),
            ("WEIR_AGENT_POLL_INTERVAL_SECS", "5"),
            ("WEIR_AGENT_BATCH_SIZE", "100"),
            ("WEIR_AGENT_STATE_FILE", "/var/lib/weir-agent/state.json"),
        ]))
        .unwrap();
        assert_eq!(cfg.events_url, "http://weir:9000/events");
        assert_eq!(cfg.poll_interval, Duration::from_secs(5));
        assert_eq!(cfg.batch_size, 100);
        assert_eq!(cfg.state_file, PathBuf::from("/var/lib/weir-agent/state.json"));
    }

    #[test]
    fn zero_batch_size_is_error() {
        let err = Config::from_source(source(&[
            ("WEIR_AGENT_BACKEND_URL", "https://b/i"),
            ("WEIR_AGENT_ORG_KEY", "k"),
            ("WEIR_AGENT_INSTANCE_ID", "i"),
            ("WEIR_AGENT_BATCH_SIZE", "0"),
        ]))
        .unwrap_err();
        assert!(err.contains("BATCH_SIZE"));
    }

    #[test]
    fn zero_poll_interval_is_error() {
        let err = Config::from_source(source(&[
            ("WEIR_AGENT_BACKEND_URL", "https://b/i"),
            ("WEIR_AGENT_ORG_KEY", "k"),
            ("WEIR_AGENT_INSTANCE_ID", "i"),
            ("WEIR_AGENT_POLL_INTERVAL_SECS", "0"),
        ]))
        .unwrap_err();
        assert!(err.contains("POLL_INTERVAL"));
    }
}
