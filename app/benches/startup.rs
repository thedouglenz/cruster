//! Cold-start micro-benchmarks for the synchronous startup path.
//!
//! Measures the work that runs before the first watch event arrives —
//! store allocation and app construction. Network and apiserver
//! latency are deliberately excluded; they belong in an integration
//! benchmark, not a CI-runnable micro-bench.

use criterion::{criterion_group, criterion_main, Criterion};
use cruster_kube::{ResourceStore, StoreRegistry};
use cruster_tui::App;
use k8s_openapi::api::core::v1::Pod;

fn bench_store_construction(c: &mut Criterion) {
    c.bench_function("store_new", |b| {
        b.iter(|| {
            let _ = ResourceStore::<Pod>::new();
        });
    });
}

fn bench_registry_construction(c: &mut Criterion) {
    c.bench_function("registry_new", |b| {
        b.iter(StoreRegistry::new);
    });
}

fn bench_app_construction(c: &mut Criterion) {
    c.bench_function("app_new", |b| {
        b.iter(|| {
            let registry = StoreRegistry::new();
            let _ = App::new(registry, None);
        });
    });
}

criterion_group!(
    benches,
    bench_store_construction,
    bench_registry_construction,
    bench_app_construction
);
criterion_main!(benches);
