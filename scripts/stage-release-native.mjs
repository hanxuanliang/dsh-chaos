#!/usr/bin/env node

import { constants } from 'node:fs'
import { copyFile, mkdir, rename, rm } from 'node:fs/promises'
import { dirname, relative, resolve } from 'node:path'
import { randomUUID } from 'node:crypto'

const [packageDirectory, sourcePath] = process.argv.slice(2)
if (packageDirectory === undefined || sourcePath === undefined) {
  throw new Error('usage: node scripts/stage-release-native.mjs <package-directory> <source-path>')
}

const root = resolve(import.meta.dirname, '..')
const packageRoot = resolve(root, packageDirectory)
const relativePackageRoot = relative(resolve(root, 'npm'), packageRoot)
if (relativePackageRoot.startsWith('..') || relativePackageRoot === '') {
  throw new Error(`native package must be inside npm/: ${packageDirectory}`)
}

const destination = resolve(packageRoot, 'dsh_chaos_core.node')
const staged = resolve(dirname(destination), `.dsh_chaos_core.${process.pid}.${randomUUID()}.tmp`)
await mkdir(dirname(destination), { recursive: true })
try {
  await copyFile(resolve(root, sourcePath), staged, constants.COPYFILE_EXCL)
  await rename(staged, destination)
} finally {
  await rm(staged, { force: true })
}
