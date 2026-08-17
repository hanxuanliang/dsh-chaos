import type { NativeActor, NativeRuntimeBinding } from './native.ts'

/** Browser-safe presentation row from the official DSH Agent Preset roster. */
export interface AgentPresetSummary {
  id: string
  trust: 'system' | 'user'
  isDefault: boolean
  name?: string
  description?: string
  broken?: string
}

/** Stable Agent identity plus its one current Session and fixed managed Workspace. */
export interface AgentProfile {
  actor: NativeActor
  binding?: NativeRuntimeBinding
  workspacePath: string
}

export interface AgentWorkspaceEntry {
  name: string
  path: string
  kind: 'directory' | 'file' | 'symlink'
  size: number
  modifiedAtMs: number
}

export interface AgentWorkspaceFile {
  path: string
  size: number
  modifiedAtMs: number
  binary: boolean
  truncated: boolean
  content?: string
}
