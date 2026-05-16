//! `cruster describe <kind>/<name>` — full info on one resource.

use crate::args::{Cli, DescribeArgs};

pub async fn run(cli: &Cli, args: &DescribeArgs) -> anyhow::Result<()> {
    let (kind, name) = parse_reference(&args.reference)?;
    let get_args = crate::args::GetArgs {
        kind: kind.into(),
        name: Some(name.into()),
        namespace: args.namespace.clone(),
        selector: None,
    };
    crate::verbs::get::run(cli, &get_args).await
}

pub fn parse_reference(s: &str) -> anyhow::Result<(&str, &str)> {
    let (kind, name) = s
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("expected kind/name (got '{s}')"))?;
    if kind.is_empty() || name.is_empty() {
        anyhow::bail!("expected kind/name (got '{s}')");
    }
    Ok((kind, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_reference() {
        assert_eq!(parse_reference("pod/nginx").unwrap(), ("pod", "nginx"));
        assert_eq!(parse_reference("deploy/web").unwrap(), ("deploy", "web"));
    }

    #[test]
    fn rejects_missing_slash() {
        assert!(parse_reference("pod-nginx").is_err());
    }

    #[test]
    fn rejects_empty_parts() {
        assert!(parse_reference("/nginx").is_err());
        assert!(parse_reference("pod/").is_err());
    }
}
