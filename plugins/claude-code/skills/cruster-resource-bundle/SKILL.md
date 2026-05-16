---
name: cruster-resource-bundle
description: Build a single self-contained markdown bundle (manifest + related events) for any Kubernetes resource. Activate when the user wants you to "look at" / "understand" / "review" a Deployment, Service, ConfigMap, Secret, Node, or Namespace — anything where a one-shot dump of context is more useful than incremental discovery.
---

# Bundle a Kubernetes resource with cruster

Cruster's `export` verb is not pod-specific. For any resource it
produces a markdown bundle containing the full manifest plus events
that involve the resource. Use this when you'd otherwise be running
`kubectl describe X && kubectl get events --field-selector ...` and
stitching the output together by hand.

## Primary call

```bash
cruster export <kind>/<name> -n <namespace>
```

Supported kinds: `pod`, `deployment`, `service`, `configmap`,
`secret`, `namespace`, `node`. Aliases (`po`, `deploy`, `svc`, `cm`,
`sec`, `ns`, `no`) are accepted.

For cluster-scoped resources (`node`, `namespace`), the `-n` flag is
ignored — pass just the name.

Write to a file with `-o <path>` if the report is for a human, or
omit it to read the markdown directly.

## Variants

Single resource, structured form (skip the markdown, get JSON):

```bash
cruster get <kind> <name> -n <namespace> --format yaml
```

Diff two resources of the same kind:

```bash
cruster diff <kind>/<name-a> <kind>/<name-b> -n <namespace>
```

## What you get in the bundle

- A header with cluster + kind/namespace/name
- The full manifest as a fenced YAML block (pruned by default)
- A `Recent events` table (only if events exist)
- For pods only: a `Recent logs` section with the last 100 lines

If the resource doesn't exist, `cruster export` exits non-zero with
a kube error — surface that to the user verbatim.

## Tip

For Secrets, cruster's TUI redacts data values; the CLI export does
**not** automatically redact. Be careful pasting Secret bundles into
chat or tickets — review before sharing.
