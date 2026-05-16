use anyhow::Context;
use cruster_kube::{
    run_watcher, ConfigMaps, Deployments, Events, Namespaces, Nodes, Pods, ResourceKind,
    ResourceStore, Secrets, Services, StoreRegistry,
};
use cruster_tui::App;
use kube::Client;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let mut argv = std::env::args_os();
    let argv0 = argv.next();
    let first_arg = argv.next();

    // CLI mode: any argument other than nothing or `tui` dispatches to
    // the CLI. Subcommand parsing happens inside cruster-cli.
    let go_cli = match first_arg.as_deref().and_then(|s| s.to_str()) {
        None => false,        // no args → TUI
        Some("tui") => false, // explicit TUI
        _ => true,
    };

    if go_cli {
        let mut full = vec![argv0.unwrap_or_default()];
        full.push(first_arg.unwrap());
        full.extend(argv);
        let code = cruster_cli::run(full).await;
        std::process::exit(code);
    }

    let client = Client::try_default()
        .await
        .context("failed to construct kube client from default kubeconfig context")?;

    let registry = StoreRegistry::new();

    spawn_watcher::<Pods>(client.clone(), registry.pods.clone());
    spawn_watcher::<Deployments>(client.clone(), registry.deployments.clone());
    spawn_watcher::<Services>(client.clone(), registry.services.clone());
    spawn_watcher::<Nodes>(client.clone(), registry.nodes.clone());
    spawn_watcher::<Events>(client.clone(), registry.events.clone());
    spawn_watcher::<ConfigMaps>(client.clone(), registry.configmaps.clone());
    spawn_watcher::<Secrets>(client.clone(), registry.secrets.clone());
    spawn_watcher::<Namespaces>(client.clone(), registry.namespaces.clone());

    let mut app = App::new(registry, Some(client));
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
