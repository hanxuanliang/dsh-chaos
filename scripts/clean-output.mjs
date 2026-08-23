#!/usr/bin/env node

import { rm } from 'node:fs/promises'
import { resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const selection = process.argv[2]
if (selection !== undefined && selection !== 'lib') {
  throw new Error(`unknown clean-output selection '${selection}'`)
}

await rm(resolve(root, 'lib'), { recursive: true, force: true })
if (selection === undefined) {
  await rm(resolve(root, 'native'), { recursive: true, force: true })
}
