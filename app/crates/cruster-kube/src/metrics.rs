//! Metrics-server integration.
//!
//! `metrics.k8s.io/v1beta1` is an aggregated API exposed by the
//! optional metrics-server addon. It's the canonical source for
//! cluster CPU/memory usage and is what `kubectl top` reads.
//!
//! We don't depend on metrics-server's Rust CRDs; instead we define
//! thin local deserialise types and hit the raw URL through
//! `kube::Client::request`. That keeps cruster usable on clusters
//! that don't have metrics-server installed (the fetcher just
//! returns an error which the caller turns into an "unavailable"
//! state).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use k8s_openapi::api::core::v1::Node;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::Client;
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::warn;

use crate::quantity::{parse_cpu, parse_memory};
use crate::store::ResourceStore;

/// Raw node usage sample returned by metrics-server. Only the fields
/// we care about are deserialised — the rest of the body is ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeMetrics {
    pub metadata: NodeMetricsMetadata,
    pub usage: NodeMetricsUsage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeMetricsMetadata {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeMetricsUsage {
    pub cpu: Quantity,
    pub memory: Quantity,
}

#[derive(Debug, Clone, Deserialize)]
struct NodeMetricsList {
    #[serde(default)]
    items: Vec<NodeMetrics>,
}

/// Shared cache populated by the metrics poller and read by the
/// dashboard. Wrapped in an `Arc<RwLock<_>>` so the poller task and
/// the render path can both touch it cheaply.
pub type SharedMetricsCache = Arc<RwLock<MetricsCache>>;

/// Cluster-wide CPU/memory utilisation, derived from metrics-server +
/// node allocatable. Three states:
///
/// - `Initializing`: poller hasn't returned yet.
/// - `Available`: latest successful sample.
/// - `Unavailable`: metrics-server returned an error (commonly: not
///   installed, returning 404). The reason is shown muted under the
///   gauges so the user understands why they're seeing a fallback.
#[derive(Debug, Clone, Default)]
pub enum MetricsCache {
    #[default]
    Initializing,
    Available {
        cpu_used_cores: f64,
        cpu_capacity_cores: f64,
        mem_used_bytes: u64,
        mem_capacity_bytes: u64,
        sampled_at: SystemTime,
    },
    Unavailable {
        reason: String,
    },
}

/// How often the poller refreshes metrics-server data.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Fetch the current `NodeMetricsList` from metrics-server. Returns
/// the per-node samples; the caller is responsible for summing into
/// cluster totals.
pub async fn fetch_node_metrics(client: &Client) -> Result<Vec<NodeMetrics>, kube::Error> {
    let req = http::Request::get("/apis/metrics.k8s.io/v1beta1/nodes")
        .body(Vec::new())
        .map_err(kube::Error::HttpError)?;
    let list: NodeMetricsList = client.request(req).await?;
    Ok(list.items)
}

/// Sum cluster-wide CPU/MEM allocatable from the nodes store. Returns
/// `(cores, bytes)`. Nodes missing `status.allocatable` contribute 0.
pub async fn sum_node_capacity(nodes: &ResourceStore<Node>) -> (f64, u64) {
    let snap = nodes.snapshot().await;
    let mut cpu = 0.0_f64;
    let mut mem = 0_u64;
    for (_, n) in snap {
        if let Some(alloc) = n.status.as_ref().and_then(|s| s.allocatable.as_ref()) {
            if let Some(q) = alloc.get("cpu") {
                if let Some(c) = parse_cpu(q) {
                    cpu += c.max(0.0);
                }
            }
            if let Some(q) = alloc.get("memory") {
                if let Some(b) = parse_memory(q) {
                    mem = mem.saturating_add(b);
                }
            }
        }
    }
    (cpu, mem)
}

/// Sum CPU + memory usage across a `NodeMetricsList`. Returns
/// `(cores, bytes)`. Quantities that fail to parse contribute 0.
pub fn sum_node_usage(samples: &[NodeMetrics]) -> (f64, u64) {
    let mut cpu = 0.0_f64;
    let mut mem = 0_u64;
    for s in samples {
        if let Some(c) = parse_cpu(&s.usage.cpu) {
            cpu += c.max(0.0);
        }
        if let Some(b) = parse_memory(&s.usage.memory) {
            mem = mem.saturating_add(b);
        }
    }
    (cpu, mem)
}

/// Background loop that refreshes `cache` every `interval` from
/// metrics-server. Designed to be `tokio::spawn`ed once at startup.
///
/// On error, writes `MetricsCache::Unavailable { reason }` so the
/// dashboard can show "metrics-server unavailable" instead of stale
/// numbers. On success, writes `MetricsCache::Available`.
pub async fn run_metrics_poller(
    client: Client,
    nodes: ResourceStore<Node>,
    cache: Arc<RwLock<MetricsCache>>,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let next = match fetch_node_metrics(&client).await {
            Ok(samples) => {
                let (used_cpu, used_mem) = sum_node_usage(&samples);
                let (cap_cpu, cap_mem) = sum_node_capacity(&nodes).await;
                MetricsCache::Available {
                    cpu_used_cores: used_cpu,
                    cpu_capacity_cores: cap_cpu,
                    mem_used_bytes: used_mem,
                    mem_capacity_bytes: cap_mem,
                    sampled_at: SystemTime::now(),
                }
            }
            Err(e) => {
                warn!(error = %e, "metrics-server fetch failed");
                MetricsCache::Unavailable {
                    reason: friendly_error(&e),
                }
            }
        };
        *cache.write().await = next;
    }
}

/// Shorter, user-facing version of a `kube::Error` for the
/// "metrics-server unavailable" message. The full error still hits
/// the tracing log via `warn!`.
fn friendly_error(e: &kube::Error) -> String {
    match e {
        kube::Error::Api(s) if s.code == 404 => "metrics-server not installed".to_string(),
        kube::Error::Api(s) => format!("metrics-server error: {}", s.message),
        other => format!("metrics-server unreachable: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialises_a_representative_payload() {
        let payload = r#"{
            "kind": "NodeMetricsList",
            "apiVersion": "metrics.k8s.io/v1beta1",
            "metadata": {},
            "items": [
                {
                    "metadata": {"name": "node-1"},
                    "timestamp": "2024-01-01T00:00:00Z",
                    "window": "30s",
                    "usage": {"cpu": "248658235n", "memory": "1819860Ki"}
                },
                {
                    "metadata": {"name": "node-2"},
                    "timestamp": "2024-01-01T00:00:00Z",
                    "window": "30s",
                    "usage": {"cpu": "100m", "memory": "2Gi"}
                }
            ]
        }"#;
        let list: NodeMetricsList = serde_json::from_str(payload).unwrap();
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].metadata.name, "node-1");
        assert_eq!(list.items[0].usage.cpu.0, "248658235n");
        assert_eq!(list.items[1].usage.memory.0, "2Gi");
    }

    #[test]
    fn missing_items_field_yields_empty_list() {
        let payload = r#"{"kind": "NodeMetricsList"}"#;
        let list: NodeMetricsList = serde_json::from_str(payload).unwrap();
        assert!(list.items.is_empty());
    }

    #[test]
    fn sum_usage_aggregates_across_nodes() {
        let payload = r#"{
            "items": [
                {"metadata": {"name": "a"}, "usage": {"cpu": "100m", "memory": "1Gi"}},
                {"metadata": {"name": "b"}, "usage": {"cpu": "200m", "memory": "2Gi"}}
            ]
        }"#;
        let list: NodeMetricsList = serde_json::from_str(payload).unwrap();
        let (cpu, mem) = sum_node_usage(&list.items);
        assert!((cpu - 0.3).abs() < 1e-9, "cpu was {cpu}");
        assert_eq!(mem, 3 * 1024 * 1024 * 1024);
    }

    #[test]
    fn sum_usage_skips_unparseable_quantities() {
        let payload = r#"{
            "items": [
                {"metadata": {"name": "a"}, "usage": {"cpu": "garbage", "memory": "1Gi"}},
                {"metadata": {"name": "b"}, "usage": {"cpu": "200m", "memory": "junk"}}
            ]
        }"#;
        let list: NodeMetricsList = serde_json::from_str(payload).unwrap();
        let (cpu, mem) = sum_node_usage(&list.items);
        assert!((cpu - 0.2).abs() < 1e-9);
        assert_eq!(mem, 1024 * 1024 * 1024);
    }

    #[tokio::test]
    async fn sum_capacity_reads_node_allocatable() {
        use k8s_openapi::api::core::v1::{Node, NodeStatus};
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        use std::collections::BTreeMap;

        let nodes: ResourceStore<Node> = ResourceStore::new();
        let mut alloc = BTreeMap::new();
        alloc.insert("cpu".to_string(), Quantity("4".to_string()));
        alloc.insert("memory".to_string(), Quantity("8Gi".to_string()));
        let node = Node {
            metadata: ObjectMeta {
                name: Some("n1".into()),
                ..Default::default()
            },
            status: Some(NodeStatus {
                allocatable: Some(alloc),
                ..Default::default()
            }),
            ..Default::default()
        };
        nodes
            .upsert(cruster_core::ResourceKey::cluster_scoped("Node", "n1"), node)
            .await;
        let (cpu, mem) = sum_node_capacity(&nodes).await;
        assert!((cpu - 4.0).abs() < 1e-9);
        assert_eq!(mem, 8 * 1024 * 1024 * 1024);
    }
}
