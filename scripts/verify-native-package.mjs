#!/usr/bin/env node

import { createRequire } from 'node:module'
import { existsSync, readFileSync, statSync } from 'node:fs'
import { resolve } from 'node:path'

const packageDirectory = process.argv[2]
if (packageDirectory === undefined) {
  throw new Error('usage: node scripts/verify-native-package.mjs <package-directory>')
}

const root = resolve(import.meta.dirname, '..')
const packageRoot = resolve(root, packageDirectory)
const manifest = JSON.parse(readFileSync(resolve(packageRoot, 'package.json'), 'utf8'))
const nativePath = resolve(packageRoot, 'dsh_chaos_core.node')
if (!existsSync(nativePath)) throw new Error(`${manifest.name} is missing dsh_chaos_core.node`)
const nativeSize = statSync(nativePath).size
if (nativeSize < 1024 * 1024 || nativeSize > 100 * 1024 * 1024) {
  throw new Error(`${manifest.name} native module has an unexpected size: ${nativeSize} bytes`)
}

const require = createRequire(import.meta.url)
const native = require(nativePath)
if (typeof native.openCollab !== 'function') {
  throw new Error(`${manifest.name} does not export openCollab`)
}
console.log(`${manifest.name}@${manifest.version}: native module loaded`)
