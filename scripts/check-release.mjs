#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const nativePackages = [
  ['darwin-arm64', '@hanxuanliang/dsh-chaos-darwin-arm64', 'darwin', 'arm64'],
  ['darwin-x64', '@hanxuanliang/dsh-chaos-darwin-x64', 'darwin', 'x64'],
  ['linux-x64-gnu', '@hanxuanliang/dsh-chaos-linux-x64-gnu', 'linux', 'x64'],
  ['win32-x64-msvc', '@hanxuanliang/dsh-chaos-win32-x64-msvc', 'win32', 'x64'],
]
const failures = []

function readJson(path) {
  return JSON.parse(readFileSync(resolve(root, path), 'utf8'))
}

function run(command, args) {
  return execFileSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'inherit'],
  }).trim()
}

const manifest = readJson('package.json')
const expectedVersion = manifest.version
const status = run('git', ['status', '--porcelain=v1', '--untracked-files=all'])
if (status !== '') failures.push('Git worktree must be clean before release')
if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME !== `v${expectedVersion}`) {
  failures.push(`release tag must be v${expectedVersion}`)
}
if (manifest.name !== '@hanxuanliang/dsh-chaos') failures.push('root package name is not @hanxuanliang/dsh-chaos')
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(expectedVersion)) failures.push('root package version is not valid semver')
if (manifest.private === true) failures.push('root package is still private')
if (manifest.publishConfig?.access !== 'public') failures.push('root package is not configured for public npm access')

const cargoManifest = readFileSync(resolve(root, 'Cargo.toml'), 'utf8')
if (!cargoManifest.includes(`version = "${expectedVersion}"`)) failures.push('Cargo workspace version does not match')
const cargoLock = readFileSync(resolve(root, 'Cargo.lock'), 'utf8')
if (!new RegExp(`name = "dsh-chaos-core"\\nversion = "${expectedVersion.replaceAll('.', '\\.')}"`).test(cargoLock)) {
  failures.push('Cargo.lock dsh-chaos-core version does not match')
}

for (const [directory, name, os, cpu] of nativePackages) {
  const path = `npm/${directory}/package.json`
  if (!existsSync(resolve(root, path))) {
    failures.push(`missing ${path}`)
    continue
  }
  const nativeManifest = readJson(path)
  if (nativeManifest.name !== name) failures.push(`${path} has the wrong package name`)
  if (nativeManifest.version !== expectedVersion) failures.push(`${path} has the wrong version`)
  if (nativeManifest.os?.[0] !== os || nativeManifest.cpu?.[0] !== cpu) {
    failures.push(`${path} has the wrong platform selector`)
  }
  if (manifest.optionalDependencies?.[name] !== `workspace:${expectedVersion}`) {
    failures.push(`root optionalDependencies does not pin ${name}`)
  }
}

for (const path of [
  'lib/index.js',
  'lib/index.d.ts',
  'lib/client.js',
  'cordis.patch.yml',
  'README.md',
  'assets/readme/hero.svg',
  `release-notes/v${expectedVersion}.md`,
]) {
  if (!existsSync(resolve(root, path))) failures.push(`missing release file ${path}`)
}

for (const stalePath of ['lib/client/atoms', 'lib/client/blocks']) {
  if (existsSync(resolve(root, stalePath))) failures.push(`stale build output remains: ${stalePath}`)
}

let packMetadata
try {
  packMetadata = JSON.parse(run('npm', ['pack', '--dry-run', '--ignore-scripts', '--json']))[0]
} catch (error) {
  failures.push(`could not inspect root npm package: ${error instanceof Error ? error.message : String(error)}`)
}
if (packMetadata !== undefined) {
  const packedFiles = new Set(packMetadata.files.map(file => file.path))
  if (packMetadata.size > 5 * 1024 * 1024) {
    failures.push(`root npm package is unexpectedly large: ${packMetadata.size} bytes`)
  }
  for (const path of ['lib/index.js', 'lib/client.js', 'assets/readme/hero.svg', `release-notes/v${expectedVersion}.md`]) {
    if (!packedFiles.has(path)) failures.push(`root npm package is missing ${path}`)
  }
  if ([...packedFiles].some(path => path.startsWith('native/'))) {
    failures.push('root npm package must not contain a platform-specific native image')
  }
  if ([...packedFiles].some(path => path.startsWith('lib/client/atoms/') || path.startsWith('lib/client/blocks/'))) {
    failures.push('root npm package contains stale pre-refactor client output')
  }
}

if (failures.length > 0) {
  console.error(['release check failed', ...failures.map(failure => `- ${failure}`)].join('\n'))
  process.exitCode = 1
} else {
  console.log(`release check passed for @hanxuanliang/dsh-chaos@${expectedVersion}`)
}
