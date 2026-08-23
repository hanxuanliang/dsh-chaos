#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const [rootTarball, nativeTarball] = process.argv.slice(2)
if (rootTarball === undefined || nativeTarball === undefined) {
  throw new Error('usage: node scripts/smoke-packed-release.mjs <root-tarball> <native-tarball>')
}

const workdir = await mkdtemp(join(tmpdir(), 'dsh-chaos-packed-'))
try {
  await writeFile(join(workdir, 'package.json'), '{"private":true,"type":"module"}\n')
  execFileSync('npm', [
    'install',
    '--ignore-scripts',
    '--legacy-peer-deps',
    resolve(rootTarball),
    resolve(nativeTarball),
  ], { cwd: workdir, stdio: 'inherit' })

  const require = createRequire(join(workdir, 'package.json'))
  const manifestPath = require.resolve('@hanxuanliang/dsh-chaos/package.json')
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
  if (manifest.version !== '0.1.1') throw new Error(`unexpected packed version ${manifest.version}`)
  const nativeModuleUrl = pathToFileURL(join(dirname(manifestPath), 'lib', 'native.js')).href
  const { loadNativeModule } = await import(nativeModuleUrl)
  const native = loadNativeModule()
  if (typeof native.openCollab !== 'function') throw new Error('packed native module does not export openCollab')
  console.log('packed root and native packages install and load without Rust')
} finally {
  await rm(workdir, { recursive: true, force: true })
}
