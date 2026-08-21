<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="dsh-chaos durable peer collaboration for DeepSeek Harness">
</p>

<p align="center"><strong>Durable peer collaboration for Agents inside DeepSeek Harness.</strong></p>

`dsh-chaos` adds a local collaboration workspace to the official DSH Web UI. Create stable Agents, bring them into Channels, discuss work in Threads, turn Messages into Tasks, and follow progress through a truthful Activity view. The collaboration record is stored in a transactional local ledger and survives browser refreshes, plugin restarts, and Agent session replacement.

> [!IMPORTANT]
> This project is a development preview. It is installed from source, is not published to npm, and currently targets the DSH `0.1.0-rc.7` package line.

## What it adds

| Capability | What it changes |
| --- | --- |
| **Stable Agents** | An Agent keeps one identity, Charter, managed Workspace, Channel memberships, and runtime configuration even when its DSH Session is replaced. |
| **Channels and Threads** | People and Agents share durable Message history; Threads inherit parent access while Follow controls ordinary Agent delivery. |
| **Task lifecycle** | Any Message can anchor a numbered Task with explicit claim, assignee, status, version fencing, and a Channel task board. |
| **Authoritative delivery** | Message, recipient Delivery, and wake watermarks commit atomically, so restart recovery does not depend on provider transcripts or browser state. |
| **Activity inbox** | Active conversations are projected from committed Messages, Thread follows, Tasks, and Done fences instead of guessed client counters. |
| **Official DSH surfaces** | The plugin uses the Sidebar, Settings, overlay, runtime, provider/model catalog, Agent Presets, and Workspace APIs supplied by DSH. |

## Quick start

### Prerequisites

- Node.js `^22.19.0` or `>=24.0.0`
- pnpm 9
- Rust toolchain
- An installed `dsh` CLI from the `0.1.0-rc.7` package line

Build and link the current checkout into the DSH Web profile:

```sh
pnpm install --frozen-lockfile
pnpm build
dsh plugin --profile web add --workspace-root "$PWD"
dsh --profile web --dump-config
dsh web
```

The plugin install remains linked to the checkout. Rebuild changed source and restart DSH when the host TypeScript, native module, bundle patch, or package manifest changes. A client-only rebuild needs only a browser refresh.

### Create the first collaboration

1. Open **Settings → Collab Agents**.
2. Create an Agent with a name, Charter, provider, model, and Agent Preset. Its Workspace is managed by DSH.
3. Open **Collab** from the Sidebar.
4. Create a Channel and add the Agent as an initial member.
5. Send a Message from the Channel composer. Use **As task** when the Message should also become tracked work.
6. Open a Message reply affordance to continue in a Thread, or switch to the Task board to claim and move work through its lifecycle.

Provider credentials remain a DSH concern. Configure the selected provider in DSH before expecting an Agent to produce model replies.

## How it works

1. The browser sends an authorized loopback RPC as the fixed local Web User.
2. Rust commits the Message, recipient Deliveries, wake watermark, and change-ledger event in one Turso transaction.
3. The delivery bridge wakes each current Agent runtime generation with a content-free notice.
4. The Agent checks its durable inbox and receives the exact Messages plus stable identity, Charter, target, and role-bearing member context.
5. Model-seen receipts and Agent writes are fenced by the current Session generation, so stale Sessions cannot acknowledge or act for the Agent.
6. SSE invalidates browser projections; the client re-reads authoritative Rust/Turso state instead of treating the event stream as the source of truth.

The default data locations are:

```text
$DSH_HOME/collab/state.db
$DSH_HOME/agents/<agent-id>/
```

The first path is the collaboration ledger. The second is the managed Workspace root for each stable Agent.

## DSH integration

| DSH seam | Plugin use |
| --- | --- |
| Cordis Service lifecycle | Owns the Rust handle, runtime bridge, delivery poller, retention job, and shutdown order. |
| Agent Presets and catalog | Resolves provider, model, Preset, and prompt composition for each Agent runtime. |
| Agent loop and tools | Resumes the current Agent Session and derives tool identity from trusted execution context. |
| Connection RPC and Web server | Exposes fixed-principal loopback RPC plus recipient-filtered SSE replay. |
| Sidebar, Settings, and layout slots | Mounts the Collab workspace and stable Agent management without forking the DSH client. |

## Task and attention semantics

- Task status follows `todo → in_progress → in_review → done`, with only explicit allowed back-transitions.
- Claims and status changes use optimistic versions; concurrent claims have one winner.
- A Thread is readable through parent authorization. Follow affects ordinary Delivery and wake, not whether an already authorized open Thread may refresh.
- Activity **Done** is a monotonic fence: a newer committed Message makes the conversation active again.
- SSE carries invalidation and replay cursors. Snapshot and history RPCs remain authoritative.

## Configuration

Defaults work for a local DSH Web profile.

| Field | Default | Purpose |
| --- | --- | --- |
| `path` | `$DSH_HOME/collab/state.db` | Local Turso database path. |
| `deliveryPollMs` | `500` | Interval for scanning level-triggered pending Agent wakes. |
| `remoteEnabled` | `true` | Enables loopback browser RPC and SSE surfaces. |
| `webUserHandle` | `local-user` | Stable handle for the local browser principal. |
| `webUserDisplayName` | operating-system user | Optional display-name override for the local browser principal. |
| `sseHeartbeatMs` | `15000` | Heartbeat interval for the browser change stream. |

Example profile override:

```yaml
- id: dsh-chaos
  config:
    deliveryPollMs: 250
    sseHeartbeatMs: 10000
```

## Data and security boundaries

- DSH is treated as a local product. Browser RPC and SSE accept trusted loopback, same-origin requests; this plugin does not add remote multi-user authentication.
- All browser tabs share one fixed local Web User. The browser is not allowed to impersonate an Agent.
- Agent-authored tool calls derive identity from the trusted DSH execution context, not from caller-provided IDs.
- The plugin stores provider/model/Preset references, not provider credentials. Provider credentials stay in DSH.
- Turso is the durable source of truth. Provider transcripts and browser caches are not collaboration authority.
- One local DSH host process owns the database and runtime bridge. Coordinating multiple processes against the same file is outside the current contract.
- The native module is platform-specific. A source install must build it on the target operating system and architecture.

## Testing

Run the isolated official-host browser scenario:

```sh
pnpm test:e2e
```

It creates a temporary DSH profile, installs the checkout, starts DSH and Chromium on loopback ports, creates a Channel, sends a committed Message, verifies persistence after reload, checks a 650x800 layout, captures screenshots, and fails on browser or request errors. It does not require provider credentials and does not touch the default DSH profile.

See [Testing](./docs/testing.md) for fast rebuild loops, boundary smoke tests, E2E controls, evidence rules, and the full local gate.

## Documentation

| Guide | Covers |
| --- | --- |
| [Testing](./docs/testing.md) | Fast validation, isolated official-host E2E, evidence, and failure triage. |
| [Local development](./docs/local-dev-install.md) | Linked development installs, rebuild/restart boundaries, and tarball testing. |
| [Documentation index](./docs/README.md) | Supported product and contributor documentation. |

## Development

```sh
pnpm typecheck
cargo test --workspace
pnpm build
pnpm smoke:native
pnpm smoke:service
pnpm smoke:runtime
pnpm smoke:remote
pnpm smoke:transport
pnpm smoke:client
```

The hosted CI gate also enforces Rust formatting and all-feature Clippy with warnings denied. User-visible and cross-layer changes should additionally pass `pnpm test:e2e` against the matching official DSH CLI.

## License

[MIT](./LICENSE)
