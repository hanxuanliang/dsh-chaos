import { isUtf8 } from 'node:buffer'
import { randomUUID } from 'node:crypto'
import { lstat, link, mkdir, open, readdir, realpath, rm } from 'node:fs/promises'
import { isAbsolute, join, relative, resolve, sep } from 'node:path'
import type { AgentWorkspaceEntry, AgentWorkspaceFile } from './agent-settings-types.ts'
import type { NativeAgentProfile } from './native.ts'

const MAX_PREVIEW_BYTES = 512 * 1024

const WORKSPACE_INSTRUCTIONS = `# Workspace continuity

This workspace is persistent and agent-owned. Its layout is intentionally unspecified; organize it as the work requires.

Keep MEMORY.md as the recovery entry point. Read it when starting or recovering work. Before long-running work, make the current objective and next action recoverable there. After a durable decision or learning, keep the relevant material and its MEMORY pointer current.

Local memory is continuity material, not shared authority. Current collaboration Messages, Tasks, Memberships, system signals, and executable evidence override conflicting local memory.
`

function initialMemory(profile: NativeAgentProfile): string {
  return `# ${profile.actor.displayName}

## Role
${profile.charter.summary}

## Key Knowledge
- No durable knowledge recorded yet.

## Active Context
- First startup.
`
}

function errno(error: unknown): string | undefined {
  return error instanceof Error && 'code' in error && typeof error.code === 'string'
    ? error.code
    : undefined
}

async function requireRegularFile(path: string): Promise<void> {
  const info = await lstat(path)
  if (info.isSymbolicLink() || !info.isFile()) {
    throw new Error(`[invalid_argument] workspace seed path is not a regular file: ${path}`)
  }
}

/** Publish a complete seed without replacing an Agent-owned existing file. */
async function seedFile(root: string, name: string, content: string): Promise<void> {
  const target = join(root, name)
  const temporary = join(root, `.chaos-seed-${randomUUID()}`)
  const file = await open(temporary, 'wx', 0o600)
  try {
    await file.writeFile(content, 'utf8')
    await file.close()
    try {
      await link(temporary, target)
    } catch (error) {
      if (errno(error) !== 'EEXIST') throw error
      await requireRegularFile(target)
    }
  } finally {
    await file.close().catch(() => {})
    await rm(temporary, { force: true })
  }
}

/** Ensure the two discoverability entry points without prescribing any layout. */
export async function initializeAgentWorkspace(
  root: string,
  profile: NativeAgentProfile,
): Promise<void> {
  await mkdir(root, { recursive: true, mode: 0o700 })
  const info = await lstat(root)
  if (info.isSymbolicLink() || !info.isDirectory()) {
    throw new Error('[invalid_argument] Agent workspace is not a regular directory')
  }
  await seedFile(root, 'AGENTS.md', WORKSPACE_INSTRUCTIONS)
  await seedFile(root, 'MEMORY.md', initialMemory(profile))
}

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
