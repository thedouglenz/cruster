---
name: cruster-recent-activity
description: Show what's been happening in a Kubernetes namespace recently — events, pod state, scaling activity. Activate when the user asks "what's wrong in <ns>" / "what's been happening in <ns>" / "anything failing right now" without naming a specific resource.
---

# Triage a namespace with cruster

When the user wants a situational read on a namespace (rather than a
specific pod), reach for events + pod listing in parallel. Both are
cheap, NDJSON-streamable, and tell different parts of the story.

## Primary calls (run in parallel)

```bash
cruster events -n <namespace> --format ndjson
cruster get pods -n <namespace> --format ndjson
```

The first surfaces what Kubernetes itself has been doing (scheduling,
pulls, restarts, evictions). The second tells you which pods are
currently in a non-Ready state.

If `<namespace>` isn't given, omit `-n` to scan all namespaces.

## Reading the output

**Events stream:** sort and prioritise by `type=Warning`. Recurring
`reason` values (`BackOff`, `FailedScheduling`,
`FailedAttachVolume`) point at systemic issues; one-off warnings are
often noise.

**Pods stream:** look at `status.phase` first
(`Pending`/`Failed`/`Succeeded` vs `Running`), then
`status.containerStatuses[*].state` for restart loops and waiting
reasons.

## When to drill deeper

If one resource keeps showing up across both streams, switch to
`cruster-investigate-pod` (for a Pod) or `cruster-resource-bundle`
(for anything else):

```bash
cruster export pod/<name> -n <namespace>
```

## Other useful narrow-scope calls

```bash
# Just CrashLoopBackOff pods cluster-wide:
cruster get pods --format ndjson | jq 'select(.status.containerStatuses[]?.state.waiting?.reason == "CrashLoopBackOff")'

# Deployments not at desired replica count:
cruster get deployments -n <namespace> --format ndjson | jq 'select(.status.readyReplicas != .spec.replicas)'

# Recent events for one resource only:
cruster events -n <namespace> --resource pod/<name> --format ndjson
```

## Tip

`cruster events` aggregates from the K8s events store, which most
clusters age out after ~1 hour. If you get no rows, that doesn't
mean nothing happened — it means nothing happened recently *and is
still in retention*. Older incidents need `kubectl get events
--all-namespaces` against a cluster with longer retention, or a log
sink.
