import { copyFile, mkdir } from 'node:fs/promises'
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

await mkdir(dirname(destination), { recursive: true })
await copyFile(source, destination)
