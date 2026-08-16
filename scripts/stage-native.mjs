import { randomUUID } from 'node:crypto'
import { constants } from 'node:fs'
import { copyFile, mkdir, rename, rm } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const profile = process.argv[2] ?? 'release'
if (profile !== 'debug' && profile !== 'release') {
  throw new Error(`unknown Cargo profile '${profile}'`)
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const extension = process.platform === 'win32'
  ? 'dll'
  : process.platform === 'darwin' ? 'dylib' : 'so'
const prefix = process.platform === 'win32' ? '' : 'lib'
const source = resolve(root, 'target', profile, `${prefix}dsh_chaos_core.${extension}`)
const destination = resolve(root, 'native', 'dsh_chaos_core.node')
const staged = resolve(
  dirname(destination),
  `.dsh_chaos_core.${String(process.pid)}.${randomUUID()}.node.tmp`,
)

await mkdir(dirname(destination), { recursive: true })
try {
  // Never truncate a native image that a live DSH process may have mmap'd.
  // Copy beside it, then atomically replace the pathname so the running
  // process keeps its old inode and the next process observes the new build.
  await copyFile(source, staged, constants.COPYFILE_EXCL)
  await rename(staged, destination)
} finally {
  await rm(staged, { force: true })
}
