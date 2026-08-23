# Releasing dsh-chaos

This project publishes one DSH plugin package and four platform-specific native packages:

- `@hanxuanliang/dsh-chaos`
- `@hanxuanliang/dsh-chaos-linux-x64-gnu`
- `@hanxuanliang/dsh-chaos-win32-x64-msvc`
- `@hanxuanliang/dsh-chaos-darwin-x64`
- `@hanxuanliang/dsh-chaos-darwin-arm64`

End users install only the main package. npm selects the matching optional native package from `os` and `cpu`; a Rust toolchain is not required.

## One-time setup

1. Make the GitHub repository public only after reviewing the tracked files and complete Git history for credentials, private URLs, personal data, and unintended large artifacts.
2. Ensure npm user `chxldxyz` can publish public packages under the `@hanxuanliang` scope and has account-level two-factor authentication enabled.
3. For the first publication, configure a short-lived granular npm automation token as the encrypted GitHub Actions secret `NPM_TOKEN`, limited to these five packages when possible. Never commit the token or print it in workflow logs.
4. After the packages exist, configure npm Trusted Publishing for `.github/workflows/release.yml`, remove the `NPM_TOKEN` secret, and revoke the bootstrap token.

The npm CLI requires a package to exist before a trusted publisher can be configured, so the first release needs the short-lived bootstrap credential. Subsequent releases should use OIDC only.

## Release contract

1. Update the root package version, Cargo workspace version, all four native package versions, lockfiles, README commands, and `release-notes/vX.Y.Z.md` in one reviewed commit.
2. Run the repository CI and `pnpm release:check` on a clean checkout.
3. Confirm the release commit is on `main`, then create and push an annotated `vX.Y.Z` tag.
4. The Release workflow builds and checks all four native modules, runs the repository release gates, then publishes the native packages followed by the root package under the `next` dist-tag.
5. Once all five npm versions are visible from the registry, the same publish job promotes them to `latest` and the release workflow is complete.

Installing the published plugin into complete DSH profiles on every operating system is an independent compatibility exercise, not a release gate. The release path already verifies each native package on its build platform and runs the repository's Rust, TypeScript, package, and smoke checks before publishing; keeping full DSH installation outside the tag workflow avoids registry/network/installer resource failures blocking an otherwise valid package release.

GitHub Releases are also independent from npm publication. The immutable Git tag remains the source-install target; create a GitHub Release separately only when downloadable release assets or a rendered release-notes page are needed.

Never reuse a native artifact across operating systems, CPU architectures, or incompatible Linux C libraries. Version `0.1.1` supports Linux x64 with glibc, Windows x64, macOS x64, and macOS arm64.

## User installation

From npm:

```sh
dsh plugin --profile web add @hanxuanliang/dsh-chaos@0.1.1
```

From the immutable GitHub tag:

```sh
npx @deepseek-ai/dsh@0.1.0-rc.7 plugin --profile web add github:hanxuanliang/dsh-chaos#v0.1.1
```

Do not document an unpinned GitHub branch as a release install target.
