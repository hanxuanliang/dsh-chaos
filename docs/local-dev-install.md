# Local development install

This guide covers a linked source install for everyday development and a tarball install for testing the plugin on another compatible machine. Commands use environment variables rather than machine-specific paths.

## Prerequisites

- Node.js `^22.19.0` or `>=24.0.0`
- pnpm 9
- Rust when building the native module
- A `dsh` CLI from the `0.1.0-rc.7` package line

Choose isolated locations before starting:

```sh
export REPO_ROOT="$(pwd)"
export DSH_HOME="$(mktemp -d)"
export PORT=3082
```

`REPO_ROOT` must point to the `dsh-chaos` checkout. `DSH_HOME` keeps the test profile and collaboration database separate from the default DSH data directory.

## Linked source install

Build the checkout and add it to the Web profile:

```sh
cd "$REPO_ROOT"
pnpm install --frozen-lockfile
pnpm build
dsh plugin --profile web add --workspace-root "$REPO_ROOT"
dsh --profile web --dump-config
dsh web --host 127.0.0.1 --port "$PORT"
```

Open `http://127.0.0.1:$PORT`. The profile remains linked to the checkout, so later builds replace the runtime artifacts without reinstalling the plugin.

### Rebuild boundaries

| Changed area | Rebuild | Apply |
| --- | --- | --- |
| `src/client/` or client CSS | `pnpm build:client` | Refresh the browser. |
| Host TypeScript | `pnpm build:ts` | Restart DSH, then refresh. |
| Rust core or N-API | `pnpm build:native:debug` | Restart DSH. |
| `cordis.patch.yml`, package metadata, schema, or client injection | Relevant full build | Restart DSH and refresh. |

For a disposable environment that performs installation, startup, browser control, assertions, and cleanup automatically, run:

```sh
pnpm test:e2e
```

See [Testing](./testing.md) for the quick loop and failure criteria.

## Tarball install

Build the tarball on the target operating system and architecture whenever possible:

```sh
cd "$REPO_ROOT"
pnpm install --frozen-lockfile
pnpm build
pnpm pack
```

Install the resulting file into an isolated Web profile:

```sh
dsh plugin --profile web add "$REPO_ROOT/dsh-chaos-0.1.0.tgz"
dsh --profile web --dump-config
dsh web --host 127.0.0.1 --port "$PORT"
```

The tarball contains a platform-specific native module. Do not reuse a build across operating systems, CPU architectures, or incompatible C libraries. A source build on the target machine is the portable fallback.

## Acceptance checks

1. The Web profile dump includes `dsh-chaos`.
2. **Collab** appears in the Sidebar and **Collab Agents** appears in Settings.
3. A Channel can be created and a committed Message remains after a browser reload.
4. The browser has no plugin load, missing-service, console, page, or request errors.
5. The repository checks pass for the layers changed in the current work.

Provider credentials remain owned by DSH and are not required for the repository E2E scenario.
