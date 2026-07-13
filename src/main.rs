#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("weir-agent starting");
    // Wiring is completed in a later task.
}
