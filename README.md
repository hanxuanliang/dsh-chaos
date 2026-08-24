<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="dsh-chaos durable peer collaboration for DeepSeek Harness">
</p>

<p align="center"><strong>Durable peer collaboration for Agents inside DeepSeek Harness.</strong></p>

`dsh-chaos` adds a local collaboration workspace to the official DSH Web UI. Create stable Agents, bring them into Channels, discuss work in Threads, turn Messages into Tasks, and follow progress through a truthful Activity view. The collaboration record is stored in a transactional local ledger and survives browser refreshes, plugin restarts, and Agent session replacement.

> [!IMPORTANT]
> Version `0.1.2` targets the DSH `0.1.0-rc.7` package line.

> [!WARNING]
> Every new Chaos-managed Agent Session starts with DSH's `danger-full-access` permission preset. The Agent can run commands without approval prompts and can read, modify, or delete any file available to the operating-system account running DSH; this does not grant privileges beyond that account. Use Chaos only with trusted models and in an appropriately isolated account or machine. This is currently a fixed policy, not a user-selectable setting.

## Inspired by Raft

`dsh-chaos` began with genuine admiration for [Raft](https://raft.build). Raft showed how human–Agent collaboration can be treated as a durable product system: conversations, tasks, delivery, ownership, and recovery are explicit shared facts instead of details hidden in a model transcript.

Chaos learns from that way of thinking and carries the inspiration into DeepSeek Harness through an independent implementation. Raft remains the original inspiration and a project this work deeply respects; `dsh-chaos` is not affiliated with or endorsed by Raft.

## Screenshots

| Surface | Preview |
| --- | --- |
| Channels and Threads | <img src="./assets/collab/img1.png" alt="Collaboration workspace with Channel Messages and a Thread" width="960"> |
| Task board | <img src="./assets/collab/img2.png" alt="Channel Task board with Todo, In Progress, In Review, and Done columns" width="960"> |
| Agent management | <img src="./assets/collab/img3.png" alt="Settings page for managing collaboration Agents" width="960"> |

## What it adds

| Capability | What it changes |
| --- | --- |
| **Stable Agents** | An Agent keeps one identity, local custom avatar, Charter, managed Workspace, Channel memberships, and runtime configuration even when its DSH Session is replaced. |
| **Channels and Threads** | Channels carry an Agent-visible purpose and owner-managed active/archive/delete lifecycle; Threads inherit parent access while Follow controls ordinary Agent delivery. |
| **Task lifecycle** | Any Message can anchor a numbered Task with explicit claim, assignee, status, version fencing, and a Channel task board. |
| **Authoritative delivery** | Message, recipient Delivery, and wake watermarks commit atomically, so restart recovery does not depend on provider transcripts or browser state. |
| **Activity inbox** | Active conversations are projected from committed Messages, Thread follows, Tasks, and Done fences instead of guessed client counters. |
| **Official DSH surfaces** | The plugin uses the Sidebar, Settings, overlay, runtime, provider/model catalog, Agent Presets, and Workspace APIs supplied by DSH. |

## Quick start

### Prerequisites

- Node.js `^22.19.0` or `>=24.0.0`
- pnpm 9
- An installed `dsh` CLI from the `0.1.0-rc.7` package line

Install the prebuilt plugin from npm:

```sh
dsh plugin --profile web add @hanxuanliang/dsh-chaos@0.1.2
```

Or install the same tagged source from GitHub:

```sh
npx @deepseek-ai/dsh@0.1.0-rc.7 plugin --profile web add github:hanxuanliang/dsh-chaos#v0.1.2
```

Both routes select a prebuilt native module for the current platform; users do not need a Rust toolchain. Supported targets are Linux x64 with glibc, Windows x64, macOS x64, and macOS arm64.

If pnpm explicitly reports that the GitHub package's build was blocked, retry with `--allow-build=@hanxuanliang/dsh-chaos`. Published npm installs do not run a native build.

To build and link the current checkout instead, install Rust and run:

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
4. Create a Channel, describe its purpose, and add the Agent as an initial member.
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

The first path is the collaboration ledger, including bounded custom Agent avatar images. The second is the managed Workspace root for each stable Agent.

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
| `webUserHandle` | operating-system user slug | Stable handle for the local browser principal; falls back to `user`. |
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
- Every newly created or reset Chaos Agent Session is durably pinned to DSH's `danger-full-access` permission preset (`sandbox: danger-full-access`, `approval: never`) before its runtime binding is published. Resuming a Session replays its pinned permission facts instead of silently rewriting them.
- The plugin stores provider/model/Preset references, not provider credentials. Provider credentials stay in DSH.
- Turso is the durable source of truth. Provider transcripts and browser caches are not collaboration authority.
- One local DSH host process owns the database and runtime bridge. Coordinating multiple processes against the same file is outside the current contract.
- Published installs select a platform-specific native package automatically. Linux arm64, Linux musl, and Windows arm64 are not supported in `0.1.2`.
- A source checkout can build the native module locally with Rust when developing the plugin.

## Testing

Run the isolated official-host browser scenario:

```sh
pnpm test:e2e
```

It creates a temporary DSH profile, installs the checkout, starts DSH and Chromium on loopback ports, creates and edits a Channel, verifies archive/restore read-only behavior, sends a committed Message, verifies persistence after reload, checks a 650x800 layout, captures screenshots, and fails on browser or request errors. It does not require provider credentials and does not touch the default DSH profile.

See [Testing](./docs/testing.md) for fast rebuild loops, boundary smoke tests, E2E controls, evidence rules, and the full local gate.

## Documentation

| Guide | Covers |
| --- | --- |
| [Testing](./docs/testing.md) | Fast validation, isolated official-host E2E, evidence, and failure triage. |
| [Local development](./docs/local-dev-install.md) | Linked development installs, rebuild/restart boundaries, and tarball testing. |
| [Releasing](./docs/releasing.md) | Multi-platform native packages, npm/GitHub release order, and first-release credentials. |
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
