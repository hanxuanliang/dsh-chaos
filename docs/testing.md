# Testing dsh-chaos

This document describes how to validate the plugin quickly against a real DSH browser surface. All commands are run from the repository root. The workflow does not require credentials and never uses the default DSH data directory.

## Test layers

The repository uses three complementary layers:

1. **Domain and migration tests** validate Rust invariants and schema upgrades.
2. **Boundary smoke tests** validate the N-API, host service, runtime, remote transport, web transport, and client bundle contracts without launching a browser.
3. **Browser E2E** installs the current checkout into an isolated official DSH profile and drives the real UI through Chrome DevTools Protocol.

No single layer replaces the others. A passing browser screenshot does not prove transaction safety, and passing unit tests do not prove that the plugin loads in the official host.

## Prerequisites

- Node.js matching `package.json`
- pnpm 9
- Rust when native code must be rebuilt
- An official `dsh` executable whose version matches the pinned DSH development packages
- Chromium or Google Chrome

The E2E runner discovers `dsh` and the browser through `PATH`. Override discovery when necessary:

```bash
export DSH_E2E_BIN="$(command -v dsh)"
export DSH_E2E_BROWSER="$(command -v chromium)"
```

The runner rejects a DSH version that differs from the repository contract. `DSH_E2E_ALLOW_VERSION_MISMATCH=1` is available only for explicit compatibility investigations; it should not be used for acceptance evidence.

## Fast feedback loop

Rebuild only the layer that changed, then run the smallest real check that crosses that boundary.

| Change | Rebuild | First validation |
|---|---|---|
| Rust collaboration core | `pnpm build:native:debug` | `pnpm smoke:native` |
| Host TypeScript or service wiring | `pnpm build:ts` | `pnpm smoke:service` |
| Runtime, tools, or delivery bridge | `pnpm build:ts` | `pnpm smoke:runtime` |
| Remote RPC or SSE | `pnpm build:ts` | `pnpm smoke:remote && pnpm smoke:transport` |
| React or CSS | `pnpm build:client` | `pnpm smoke:client` |
| Cross-layer or user-visible flow | changed layers only | `pnpm test:e2e:run` |

`test:e2e:run` assumes the required build outputs already exist. Use it while iterating. Use `pnpm test:e2e` for a clean E2E acceptance run because it rebuilds native debug, host TypeScript, and the client bundle first.

## Real browser E2E

Run the core scenario:

```bash
pnpm test:e2e
```

The runner performs the following work automatically:

1. creates a temporary `DSH_HOME`, browser profile, configuration directory, and cache directory;
2. installs the current checkout into the temporary official web profile through the DSH plugin command;
3. starts DSH and Chromium on free loopback ports;
4. dismisses first-run screens without configuring a provider credential;
5. opens the collaboration workspace;
6. creates a Channel through the real UI and RPC path;
7. sends a Message through the real composer and RPC path;
8. reloads the page and verifies that the Channel and Message persisted;
9. switches to a 650x800 viewport and verifies that the Channel remains usable;
10. fails on console errors, uncaught page exceptions, failed requests, or relevant HTTP error responses;
11. captures wide and narrow screenshots and removes the temporary environment.

Artifacts are written under `artifacts/e2e/<timestamp>/` and are ignored by Git. A failed run also writes `failure.png` when the page is available and `failure.txt` with the runner and child-process diagnostics.

Useful controls:

```bash
# Show the browser while the scenario runs.
DSH_E2E_HEADFUL=1 pnpm test:e2e:run

# Preserve the isolated DSH and browser profiles for investigation.
DSH_E2E_KEEP=1 pnpm test:e2e:run

# Print DSH and browser process output while running.
DSH_E2E_DEBUG=1 pnpm test:e2e:run

# Write evidence to another repository-relative directory.
DSH_E2E_ARTIFACTS=artifacts/custom-run pnpm test:e2e:run
```

The core scenario is intentionally short. Feature-specific acceptance should extend it only when the assertion is stable, user-visible, and backed by the real host. Large visual matrices belong in an explicit release or review run rather than the default two-minute loop.

## Fast CDP spot checks

When DSH and Chromium are already running with remote debugging enabled, `scripts/cdp.mjs` provides small inspection commands:

```bash
export CDP_DEBUG=http://127.0.0.1:9222
node scripts/cdp.mjs eval 'document.title'
node scripts/cdp.mjs click Collab
node scripts/cdp.mjs shot artifacts/spot-check.png
```

These commands are useful for diagnosis and visual iteration. They are not a replacement for the E2E runner because each invocation is independent and does not own process lifecycle or aggregate browser failures.

## Full local gate

Before pushing a change, reproduce the hosted checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm typecheck
pnpm build:native:debug
pnpm build:ts
pnpm build:client
pnpm smoke:native
pnpm smoke:service
pnpm smoke:runtime
pnpm smoke:remote
pnpm smoke:transport
pnpm smoke:client
git diff --check
```

Run `pnpm test:e2e` in addition to this gate for any user-visible, persistence, navigation, responsive, or cross-layer change. Report smoke-only validation as smoke validation, not as E2E acceptance.

## Failure triage

1. Read `failure.txt` and inspect `failure.png`.
2. Re-run with `DSH_E2E_DEBUG=1` to expose host and browser logs.
3. Re-run with `DSH_E2E_HEADFUL=1 DSH_E2E_KEEP=1` when interactive inspection is required.
4. Use `scripts/cdp.mjs` against the preserved browser debugging endpoint for targeted DOM or computed-style checks.
5. Rebuild the changed layer before treating a repeated failure as a product defect.
