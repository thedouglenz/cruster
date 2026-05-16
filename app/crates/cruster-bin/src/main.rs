use anyhow::Context;
use cruster_kube::{
    Deployments, Events, Nodes, Pods, ResourceKind, ResourceStore, Services, StoreRegistry,
    run_watcher,
};
use cruster_tui::App;
use kube::Client;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let client = Client::try_default()
        .await
        .context("failed to construct kube client from default kubeconfig context")?;

    let registry = StoreRegistry::new();

    // Spawn one watcher per kind. (More kinds wired up in subsequent tasks.)
    spawn_watcher::<Pods>(client.clone(), registry.pods.clone());
    spawn_watcher::<Deployments>(client.clone(), registry.deployments.clone());
    spawn_watcher::<Services>(client.clone(), registry.services.clone());
    spawn_watcher::<Nodes>(client.clone(), registry.nodes.clone());
    spawn_watcher::<Events>(client.clone(), registry.events.clone());

    let mut app = App::new(registry);
    app.run().await
}

fn spawn_watcher<K: ResourceKind>(client: Client, store: ResourceStore<K::Object>) {
    tokio::spawn(async move {
        if let Err(e) = run_watcher::<K>(client, store).await {
            tracing::error!(kind = K::name(), error = %e, "watcher exited");
        }
    });
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
}
