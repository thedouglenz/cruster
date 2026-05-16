# cruster app

Rust workspace for the cruster TUI.

## Build

```sh
cargo build
```

## Run

```sh
cargo run -p cruster-bin
```

Uses your active kubeconfig context.

## Test

```sh
cargo test --workspace
```

## Keybindings (v1)

### Navigation
| Key | Action |
|---|---|
| `j` / `↓` | move selection down |
| `k` / `↑` | move selection up |
| `g` / `Home` | top |
| `G` / `End` | bottom |

### Switching kinds
Type `:` to enter command mode, then a kind alias and `Enter`.

| Alias | Kind |
|---|---|
| `po` / `pods` | Pods |
| `deploy` / `deployments` | Deployments |
| `svc` / `services` | Services |
| `no` / `nodes` | Nodes |
| `ev` / `events` | Events |
| `cm` / `configmaps` | ConfigMaps |
| `sec` / `secrets` | Secrets |
| `ns` / `namespaces` | Namespaces |

### Actions on selection
| Key | Action |
|---|---|
| `d` / `y` | describe (YAML pane); inside pane: `j`/`k` to scroll, `Esc` closes |
| `e` | edit YAML in `$EDITOR` (`$VISUAL` → `$EDITOR` → `vi`) and `kubectl apply` (disabled for Secrets in v1) |
| `l` | tail logs (Pods only); inside pane: `/` enters grep, `j`/`k` scroll, `Esc` closes |
| `s` | exec into pod (Pods only); suspends TUI, runs `kubectl exec -it -- $SHELL`, restores on exit |
| `f` | port-forward (Pods only); prompts for `local:remote` mapping; forwards are cancelled on `q` |

### Exit
| Key | Action |
|---|---|
| `q` / `Esc` (in main view) | quit |
