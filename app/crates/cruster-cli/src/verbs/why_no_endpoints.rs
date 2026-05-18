//! `cruster why-no-endpoints <service>` — diagnose services with no live endpoints.
//!
//! Fetches the Service, matching Pods, and EndpointSlices, then runs a
//! structural diagnostic to explain why no endpoints are ready.

use std::io::Write;

use anyhow::{bail, Context as _};
use k8s_openapi::api::core::v1::{Pod, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use kube::api::ListParams;
use kube::{Api, Client};
use serde_json::json;

use crate::args::{Cli, WhyNoEndpointsArgs};
use crate::output::{effective_format, stdout_is_tty};
use cruster_core::diagnose::endpoints::diagnose;

pub async fn run(cli: &Cli, args: &WhyNoEndpointsArgs) -> anyhow::Result<()> {
    let client = Client::try_default().await?;

    let namespace = match &args.namespace {
        Some(ns) => ns.clone(),
        None => client.default_namespace().to_string(),
    };

    let svc_api: Api<Service> = Api::namespaced(client.clone(), &namespace);
    let service = svc_api.get(&args.service).await.with_context(|| {
        format!(
            "service {} not found in namespace {}",
            args.service, namespace
        )
    })?;

    let pod_api: Api<Pod> = Api::namespaced(client.clone(), &namespace);
    let pods = pod_api.list(&ListParams::default()).await?.items;

    let slice_api: Api<EndpointSlice> = Api::namespaced(client.clone(), &namespace);
    let slices: Vec<EndpointSlice> = slice_api
        .list(
            &ListParams::default().labels(&format!("kubernetes.io/service-name={}", args.service)),
        )
        .await?
        .items;

    let result = match diagnose(&service, &pods, &slices) {
        Ok(diag) => diag,
        Err(has_endpoints) => {
            let err = json!({
                "error": format!(
                    "service has {} ready endpoints; nothing to diagnose",
                    has_endpoints.ready_endpoints
                )
            });
            let format = effective_format(cli.format, cli.llm, stdout_is_tty());
            let mut stdout = std::io::stdout().lock();
            match format {
                crate::args::Format::Text => {
                    writeln!(
                        stdout,
                        "{}: has {} ready endpoints; nothing to diagnose",
                        args.service, has_endpoints.ready_endpoints
                    )?;
                }
                _ => {
                    writeln!(stdout, "{}", serde_json::to_string(&err)?)?;
                }
            }
            bail!(
                "service has {} ready endpoints; nothing to diagnose",
                has_endpoints.ready_endpoints
            );
        }
    };

    let format = effective_format(cli.format, cli.llm, stdout_is_tty());
    let mut stdout = std::io::stdout().lock();

    match format {
        crate::args::Format::Text => {
            let matching = result.evidence.matching_pods.unwrap_or(0);
            let ready = result.evidence.ready_pods.unwrap_or(0);
            writeln!(
                stdout,
                "{}: {} - {}/{} of selected pods are Ready",
                result.service,
                result.reason.as_str(),
                ready,
                matching
            )?;
            if let Some(fix) = &result.suggested_fix {
                writeln!(stdout, "Suggested fix: {}", fix)?;
            }
        }
        crate::args::Format::Json => {
            writeln!(stdout, "{}", serde_json::to_string_pretty(&result)?)?;
        }
        crate::args::Format::Ndjson => {
            writeln!(stdout, "{}", serde_json::to_string(&result)?)?;
        }
        crate::args::Format::Yaml => {
            writeln!(stdout, "{}", serde_yaml::to_string(&result)?)?;
        }
    }

    Ok(())
}
