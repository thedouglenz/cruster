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

## Keybindings

### Navigation
| Key | Action |
|---|---|
| `j` / `↓` | move selection down |
| `k` / `↑` | move selection up |
| `g` / `Home` | top |
| `G` / `End` | bottom |

### Discoverability
| Key | Action |
|---|---|
| `Ctrl+P` | command palette (fuzzy match over all actions + all kinds) |
| `/` | search / filter (token syntax: `ns:prod status:Running ~name`) |
| `H` | recents palette (recency × frequency ranked) |
| `r` | relationships (jump to related resource — owner, mounted cm/secret, service, node) |
| `W` | run a saved workflow from `~/.config/cruster/workflows/*.toml` |
| `T` | theme palette (9 bundled themes; switching to non-default is Pro) |
| `Tab` | cycle focus between view ↔ open pane(s); focused pane title shows `◉` |

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
| `e` | edit YAML in `$EDITOR` (`$VISUAL` → `$EDITOR` → `vi`) and `kubectl apply` (disabled for Secrets in v1; gated by read-only) |
| `l` | tail logs (Pods only); inside pane: `/` enters grep, `j`/`k` scroll, `Esc` closes |
| `s` | exec into pod (Pods only, must be Running); suspends TUI |
| `f` | port-forward (Pods only); prompts for `local:remote`; gated by read-only |
| `K` | copy the describe `kubectl` equivalent to clipboard |
| `P` then `d`/`w`/`s` | **(Pro)** render a prompt template + copy to clipboard. Shipped: `d` diagnose, `w` why-failing, `s` summarize-events. User templates: `~/.config/cruster/prompts/*.toml` |
| `E` | **(Pro)** export a diagnostic markdown bundle to `./<kind>-<name>-<ts>.md` |

### Safety
The top status bar shows `[<context>] <env> <ro|rw>`. Environment is
classified from `~/.config/cruster/safety.toml`:

```toml
[matchers]
prod = ["^prod-", "production"]
staging = ["^staging-", "stg-"]
dev = ["^dev-"]
local = ["^k3d-", "^kind-", "^minikube"]
```

Prod contexts start in read-only mode and **cannot** be toggled out of
read-only in this session (relaunch with a future `--rw` flag to
override). Other environments toggle with `Ctrl+R`.

### Exit
| Key | Action |
|---|---|
| `q` / `Esc` (in main view) | quit |
