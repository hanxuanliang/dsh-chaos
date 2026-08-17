import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react'
import type { InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {
  AgentPresetSummary,
  AgentProfile,
  AgentWorkspaceEntry,
  AgentWorkspaceFile,
} from '../agent-settings-types.ts'
import type { NativeRuntimeBinding } from '../native.ts'
import type { ChaosPanelInjected } from './ChaosPanel.tsx'
import css from './ChaosPanel.module.css'

export type AgentsSettingsProps = PropsRuntime<'settings.section'> & InjectFace<ChaosPanelInjected> & {
  close?: () => void
}

type AgentTab = 'profile' | 'workspace'

function moveAgentTab(event: ReactKeyboardEvent<HTMLButtonElement>, setTab: (tab: AgentTab) => void): void {
  const next = event.key === 'ArrowRight' || event.key === 'End'
    ? 'workspace'
    : event.key === 'ArrowLeft' || event.key === 'Home'
      ? 'profile'
      : undefined
  if (next === undefined) return
  event.preventDefault()
  setTab(next)
  document.getElementById(`dsh-chaos-agent-${next}-tab`)?.focus()
}

function presetLabel(preset: AgentPresetSummary | undefined, id: string | undefined): string {
  if (preset === undefined) return id ?? '未绑定'
  return preset.name ?? preset.id
}

function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${String(bytes)} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function ProfileField({ label, value, mono = false }: { label: string; value: string; mono?: boolean }): ReactNode {
  return (
    <div className={css.agentProfileField}>
      <span>{label}</span>
      <strong data-mono={mono || undefined} title={value}>{value}</strong>
    </div>
  )
}

function AgentProfileView({
  profile,
  binding,
  preset,
  loading,
  error,
  onOpen,
}: {
  profile: AgentProfile | undefined
  binding: NativeRuntimeBinding | undefined
  preset: AgentPresetSummary | undefined
  loading: boolean
  error: string | null
  onOpen: () => void
}): ReactNode {
  if (loading && profile === undefined) return <p className={css.empty}>正在读取 Agent…</p>
  if (error !== null) return <div className={css.dialogError} role="alert">{error}</div>
  if (profile === undefined) return <p className={css.empty}>选择一个 Agent 查看详情。</p>
  return (
    <div
      id="dsh-chaos-agent-profile-panel"
      className={css.agentProfileView}
      role="tabpanel"
      aria-labelledby="dsh-chaos-agent-profile-tab"
    >
      <div className={css.agentProfileHero}>
        <div>
          <strong>{profile.actor.displayName}</strong>
          <span>@{profile.actor.handle}</span>
        </div>
        <button
          type="button"
          className={css.primaryButton}
          disabled={binding === undefined}
          onClick={onOpen}
        >
          打开 Session
        </button>
      </div>
      <div className={css.agentProfileGrid}>
        <ProfileField label="Preset" value={presetLabel(preset, binding?.preset)} />
        <ProfileField label="Runtime" value={binding?.provider ?? '未绑定'} />
        <ProfileField label="Model" value={binding?.model ?? '未绑定'} />
        <ProfileField label="Session" value={binding?.sessionId ?? '未绑定'} mono />
        <ProfileField label="Workspace" value={profile.workspacePath} mono />
      </div>
      {preset?.description !== undefined && (
        <p className={css.agentPresetDescription}>{preset.description}</p>
      )}
    </div>
  )
}

function WorkspaceTree({
  agentId,
  list,
  read,
}: {
  agentId: string
  list: ChaosPanelInjected['listAgentWorkspace']
  read: ChaosPanelInjected['readAgentWorkspaceFile']
}): ReactNode {
  const [includeHidden, setIncludeHidden] = useState(false)
  const [byDirectory, setByDirectory] = useState<Record<string, AgentWorkspaceEntry[]>>({})
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set())
  const [preview, setPreview] = useState<AgentWorkspaceFile | undefined>()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const epoch = useRef(0)

  useEffect(() => {
    const current = ++epoch.current
    setByDirectory({})
    setExpanded(new Set())
    setPreview(undefined)
    setError(null)
    setLoading(true)
    void list(agentId, '', includeHidden).then(
      entries => {
        if (epoch.current !== current) return
        setByDirectory({ '': entries })
        setLoading(false)
      },
      failure => {
        if (epoch.current !== current) return
        setError(failure instanceof Error ? failure.message : String(failure))
        setLoading(false)
      },
    )
  }, [agentId, includeHidden, list])

  const toggleDirectory = (path: string): void => {
    if (expanded.has(path)) {
      setExpanded(current => {
        const next = new Set(current)
        next.delete(path)
        return next
      })
      return
    }
    setExpanded(current => new Set(current).add(path))
    if (byDirectory[path] !== undefined) return
    const current = epoch.current
    void list(agentId, path, includeHidden).then(
      entries => {
        if (epoch.current !== current) return
        setByDirectory(value => ({ ...value, [path]: entries }))
      },
      failure => {
        if (epoch.current !== current) return
        setError(failure instanceof Error ? failure.message : String(failure))
      },
    )
  }

  const openFile = (path: string): void => {
    const current = epoch.current
    setError(null)
    void read(agentId, path).then(
      value => {
        if (epoch.current === current) setPreview(value)
      },
      failure => {
        if (epoch.current !== current) return
        setError(failure instanceof Error ? failure.message : String(failure))
      },
    )
  }

  const renderEntries = (directory: string, depth: number): ReactNode => {
    const entries = byDirectory[directory]
    if (entries === undefined) return <p className={css.workspaceLoading}>正在读取…</p>
    return entries.map(entry => (
      <div key={entry.path}>
        <button
          type="button"
          className={css.workspaceEntry}
          style={{ paddingInlineStart: `${String(10 + depth * 16)}px` }}
          data-selected={preview?.path === entry.path || undefined}
          disabled={entry.kind === 'symlink'}
          title={entry.kind === 'symlink' ? '为保证 Agent Workspace 边界，不打开符号链接' : entry.path}
          onClick={() => {
            if (entry.kind === 'directory') toggleDirectory(entry.path)
            else if (entry.kind === 'file') openFile(entry.path)
          }}
        >
          <span aria-hidden>{entry.kind === 'directory' ? (expanded.has(entry.path) ? '⌄' : '›') : entry.kind === 'file' ? '·' : '↗'}</span>
          <strong>{entry.name}</strong>
          {entry.kind === 'file' && <small>{sizeLabel(entry.size)}</small>}
        </button>
        {entry.kind === 'directory' && expanded.has(entry.path) && renderEntries(entry.path, depth + 1)}
      </div>
    ))
  }

  return (
    <div
      id="dsh-chaos-agent-workspace-panel"
      className={css.workspaceView}
      role="tabpanel"
      aria-labelledby="dsh-chaos-agent-workspace-tab"
    >
      <div className={css.workspaceToolbar}>
        <span>固定目录，仅展示</span>
        <label>
          <input
            type="checkbox"
            checked={includeHidden}
            onChange={event => { setIncludeHidden(event.target.checked) }}
          />
          显示隐藏文件
        </label>
      </div>
      {error !== null && <div className={css.dialogError} role="alert">{error}</div>}
      <div className={css.workspaceSplit}>
        <nav className={css.workspaceTree} aria-label="Agent Workspace 文件">
          {loading ? <p className={css.empty}>正在读取 Workspace…</p> : renderEntries('', 0)}
        </nav>
        <section className={css.workspacePreview} aria-label="文件预览">
          {preview === undefined ? (
            <p className={css.empty}>选择文件预览。</p>
          ) : preview.binary ? (
            <p className={css.empty}>二进制文件 · {sizeLabel(preview.size)}</p>
          ) : (
            <>
              <header>
                <strong title={preview.path}>{preview.path}</strong>
                <span>{sizeLabel(preview.size)}</span>
              </header>
              {preview.truncated && <p className={css.workspaceNotice}>仅显示前 512 KB。</p>}
              <pre>{preview.content}</pre>
            </>
          )}
        </section>
      </div>
    </div>
  )
}

/** Agent Settings: preset-aware creation plus the fixed Profile / Workspace projection. */
export function AgentsSettings(props: AgentsSettingsProps): ReactNode {
  const state = props.useChaos(value => value)
  const agents = state.actors.filter(actor => actor.kind === 'agent')
  const usablePresets = state.agentPresets.filter(preset => preset.broken === undefined)
  const [name, setName] = useState('')
  const [presetId, setPresetId] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [selectedAgentId, setSelectedAgentId] = useState<string | undefined>()
  const [tab, setTab] = useState<AgentTab>('profile')
  const [profile, setProfile] = useState<AgentProfile | undefined>()
  const [profileLoading, setProfileLoading] = useState(false)
  const [profileError, setProfileError] = useState<string | null>(null)
  const profileEpoch = useRef(0)

  const selectedActor = agents.find(actor => actor.id === selectedAgentId)
  const selectedBinding = state.bindings.find(binding => binding.agentId === selectedAgentId)
  const selectedPreset = state.agentPresets.find(preset => preset.id === selectedBinding?.preset)
  const createPreset = state.agentPresets.find(preset => preset.id === presetId)

  useEffect(() => { void props.ensure() }, [props.ensure])
  useEffect(() => {
    document.documentElement.dataset.chaosAgentSettings = 'open'
    return () => { delete document.documentElement.dataset.chaosAgentSettings }
  }, [])

  useEffect(() => {
    if (usablePresets.some(preset => preset.id === presetId)) return
    setPresetId(usablePresets.find(preset => preset.isDefault)?.id ?? usablePresets[0]?.id ?? '')
  }, [presetId, usablePresets])

  useEffect(() => {
    if (selectedAgentId !== undefined && agents.some(agent => agent.id === selectedAgentId)) return
    setSelectedAgentId(agents[0]?.id)
  }, [agents, selectedAgentId])

  useEffect(() => {
    if (selectedAgentId === undefined) {
      setProfile(undefined)
      setProfileError(null)
      return
    }
    const current = ++profileEpoch.current
    setProfile(undefined)
    setProfileError(null)
    setProfileLoading(true)
    void props.readAgentProfile(selectedAgentId).then(
      value => {
        if (profileEpoch.current !== current) return
        setProfile(value)
        setProfileLoading(false)
      },
      failure => {
        if (profileEpoch.current !== current) return
        setProfileError(failure instanceof Error ? failure.message : String(failure))
        setProfileLoading(false)
      },
    )
  }, [props.readAgentProfile, selectedAgentId])

  const presetById = useMemo(
    () => new Map(state.agentPresets.map(preset => [preset.id, preset])),
    [state.agentPresets],
  )

  const create = (event: FormEvent): void => {
    event.preventDefault()
    const next = name.trim()
    if (next === '' || presetId === '' || pending) return
    setPending(true)
    setError(null)
    void props.createAgent(next, presetId, false).then(
      created => {
        setName('')
        setSelectedAgentId(created.actor.id)
        setTab('profile')
        setPending(false)
      },
      failure => {
        setPending(false)
        setError(failure instanceof Error ? failure.message : String(failure))
      },
    )
  }

  return (
    <section className={css.settingsPage} aria-label="Agents">
      <div className={css.agentSettingsIntro}>
        <div>
          <strong>Agents</strong>
          <p>一个 Agent 对应一个 Session 和固定 Workspace。</p>
        </div>
        <form className={css.settingsCreate} onSubmit={create}>
          <label>
            <span>名称</span>
            <input
              value={name}
              onChange={event => { setName(event.target.value) }}
              placeholder="Agent 名称"
            />
          </label>
          <label>
            <span>Preset</span>
            <select
              value={presetId}
              disabled={usablePresets.length === 0}
              onChange={event => { setPresetId(event.target.value) }}
            >
              {state.agentPresets.map(preset => (
                <option key={preset.id} value={preset.id} disabled={preset.broken !== undefined}>
                  {presetLabel(preset, preset.id)}{preset.isDefault ? ' · 默认' : ''}{preset.broken !== undefined ? ' · 不可用' : ''}
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className={css.primaryButton} disabled={pending || name.trim() === '' || presetId === ''}>
            {pending ? '创建中…' : '创建 Agent'}
          </button>
        </form>
        {createPreset?.description !== undefined && <p className={css.agentPresetDescription}>{createPreset.description}</p>}
        {error !== null && <div className={css.dialogError} role="alert">{error}</div>}
      </div>

      <div className={css.agentSettingsBody}>
        <nav className={css.settingsList} aria-label="Agents">
          {agents.length === 0 && <p className={css.empty}>还没有 Agent。</p>}
          {agents.map(actor => {
            const binding = state.bindings.find(item => item.agentId === actor.id)
            const preset = presetById.get(binding?.preset ?? '')
            return (
              <button
                key={actor.id}
                type="button"
                className={css.settingsRow}
                data-selected={actor.id === selectedAgentId || undefined}
                aria-pressed={actor.id === selectedAgentId}
                onClick={() => { setSelectedAgentId(actor.id) }}
              >
                <strong>{actor.displayName}</strong>
                <span>{presetLabel(preset, binding?.preset)}</span>
              </button>
            )
          })}
        </nav>

        <div className={css.agentSettingsDetail}>
          {selectedActor === undefined ? (
            <p className={css.empty}>创建或选择一个 Agent。</p>
          ) : (
            <>
              <div className={css.agentTabs} role="tablist" aria-label={`${selectedActor.displayName} 详情`}>
                <button
                  id="dsh-chaos-agent-profile-tab"
                  type="button"
                  role="tab"
                  aria-selected={tab === 'profile'}
                  aria-controls="dsh-chaos-agent-profile-panel"
                  tabIndex={tab === 'profile' ? 0 : -1}
                  onClick={() => { setTab('profile') }}
                  onKeyDown={event => { moveAgentTab(event, setTab) }}
                >Profile</button>
                <button
                  id="dsh-chaos-agent-workspace-tab"
                  type="button"
                  role="tab"
                  aria-selected={tab === 'workspace'}
                  aria-controls="dsh-chaos-agent-workspace-panel"
                  tabIndex={tab === 'workspace' ? 0 : -1}
                  onClick={() => { setTab('workspace') }}
                  onKeyDown={event => { moveAgentTab(event, setTab) }}
                >Workspace</button>
              </div>
              {tab === 'profile' ? (
                <AgentProfileView
                  profile={profile}
                  binding={selectedBinding}
                  preset={selectedPreset}
                  loading={profileLoading}
                  error={profileError}
                  onOpen={() => {
                    props.openAgent(selectedActor.id)
                    props.close?.()
                  }}
                />
              ) : (
                <WorkspaceTree
                  agentId={selectedActor.id}
                  list={props.listAgentWorkspace}
                  read={props.readAgentWorkspaceFile}
                />
              )}
            </>
          )}
        </div>
      </div>
    </section>
  )
}
