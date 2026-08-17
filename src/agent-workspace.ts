import { isUtf8 } from 'node:buffer'
import { lstat, open, readdir, realpath } from 'node:fs/promises'
import { isAbsolute, join, relative, resolve, sep } from 'node:path'
import type { AgentWorkspaceEntry, AgentWorkspaceFile } from './agent-settings-types.ts'

const MAX_PREVIEW_BYTES = 512 * 1024

function workspaceSegments(path: string): string[] {
  if (path === '') return []
  if (isAbsolute(path) || path.includes('\\') || path.includes('\0')) {
    throw new Error('[invalid_argument] workspace path must be relative')
  }
  const segments = path.split('/')
  if (segments.some(segment => segment === '' || segment === '.' || segment === '..')) {
    throw new Error('[invalid_argument] workspace path contains an invalid segment')
  }
  return segments
}

function inside(root: string, candidate: string): boolean {
  return candidate === root || candidate.startsWith(`${root}${sep}`)
}

async function safeWorkspacePath(root: string, path: string): Promise<{ absolute: string; rootReal: string }> {
  const rootReal = await realpath(root)
  const absolute = resolve(root, ...workspaceSegments(path))
  if (!inside(resolve(root), absolute)) throw new Error('[invalid_argument] workspace path escapes its Agent root')
  const info = await lstat(absolute)
  if (info.isSymbolicLink()) throw new Error('[invalid_argument] workspace symlinks cannot be opened')
  const candidateReal = await realpath(absolute)
  if (!inside(rootReal, candidateReal)) throw new Error('[invalid_argument] workspace path escapes its Agent root')
  return { absolute: candidateReal, rootReal }
}

/** List one directory without following symlinks outside the managed Agent Workspace. */
export async function listAgentWorkspace(
  root: string,
  dirPath: string,
  includeHidden: boolean,
): Promise<AgentWorkspaceEntry[]> {
  const { absolute, rootReal } = await safeWorkspacePath(root, dirPath)
  const directory = await lstat(absolute)
  if (!directory.isDirectory()) throw new Error('[invalid_argument] workspace path is not a directory')
  const rows = await readdir(absolute, { withFileTypes: true })
  const entries = await Promise.all(rows
    .filter(row => includeHidden || !row.name.startsWith('.'))
    .map(async row => {
      const entryPath = join(absolute, row.name)
      const info = await lstat(entryPath)
      const path = relative(rootReal, entryPath).split(sep).join('/')
      const kind = info.isSymbolicLink()
        ? 'symlink'
        : info.isDirectory()
          ? 'directory'
          : 'file'
      return {
        name: row.name,
        path,
        kind,
        size: info.size,
        modifiedAtMs: info.mtimeMs,
      } satisfies AgentWorkspaceEntry
    }))
  return entries.sort((left, right) => {
    const leftRank = left.kind === 'directory' ? 0 : left.kind === 'file' ? 1 : 2
    const rightRank = right.kind === 'directory' ? 0 : right.kind === 'file' ? 1 : 2
    return leftRank - rightRank || left.name.localeCompare(right.name)
  })
}

/** Read a bounded UTF-8 preview; binary and oversized files remain metadata-only. */
export async function readAgentWorkspaceFile(root: string, path: string): Promise<AgentWorkspaceFile> {
  const { absolute } = await safeWorkspacePath(root, path)
  const info = await lstat(absolute)
  if (!info.isFile()) throw new Error('[invalid_argument] workspace path is not a file')
  const truncated = info.size > MAX_PREVIEW_BYTES
  const file = await open(absolute, 'r')
  let preview: Buffer
  try {
    preview = Buffer.alloc(Math.min(info.size, MAX_PREVIEW_BYTES))
    let offset = 0
    while (offset < preview.length) {
      const { bytesRead } = await file.read(preview, offset, preview.length - offset, offset)
      if (bytesRead === 0) break
      offset += bytesRead
    }
    preview = preview.subarray(0, offset)
  } finally {
    await file.close()
  }
  const binary = !isUtf8(preview) || preview.includes(0)
  return {
    path,
    size: info.size,
    modifiedAtMs: info.mtimeMs,
    binary,
    truncated,
    ...binary ? {} : { content: preview.toString('utf8') },
  }
}
