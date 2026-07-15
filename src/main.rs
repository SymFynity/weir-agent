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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
