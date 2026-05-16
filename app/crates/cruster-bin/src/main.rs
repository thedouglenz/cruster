use anyhow::Context;
use cruster_kube::{run_pod_watcher, ResourceStore};
use cruster_tui::App;
use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let client = Client::try_default()
        .await
        .context("failed to construct kube client from default kubeconfig context")?;

    let store = ResourceStore::<Pod>::new();
    let watcher_store = store.clone();
    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = run_pod_watcher(client, watcher_store).await {
            tracing::error!(error = %e, "pod watcher exited");
        }
    });

    let mut app = App::new(store);
    let app_result = app.run().await;

    watcher_handle.abort();
    app_result
}

fn init_tracing() {
    // Log to stderr only — stdout is owned by the TUI. Default off
    // unless RUST_LOG is set, so a normal run does not pollute the
    // terminal.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}
