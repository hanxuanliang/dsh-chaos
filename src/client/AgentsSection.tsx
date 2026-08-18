/**
 * Agents settings section (slot `settings.section`, id `dsh-chaos-agents`):
 * the management page for collaboration Agents — list, drill-in detail, and
 * the create form. All state is local React state; the page has no store and
 * no relation to the dock's ChaosClientController lifecycle.
 *
 * Data sources: the plugin's own `/dsh-chaos` RPC channel (actors /
 * runtime.bindings / agent.profile / agent.create / agent.delete, unwrapped
 * twice — transport RpcResult, then CollabDomainResult) plus the host model
 * catalog (`connection.api.llm.providers/models`, single RpcResponse unwrap).
 *
 * Visual language mirrors SubscriptionsSection: one bordered card per item,
 * pill buttons, inline style objects, every color a `--dsw-alias-*` token.
 * Copy is hardcoded Chinese, like the rest of this client.
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import type {
  ClientConnectionRpc,
  ConfigurableProviderView,
  IApiClient,
  ModelCatalogModel,
  ModelProviderGroup,
} from '@deepseek-ai/dsh-client-connection/client'
import {
  IconAgentPresetOutline16,
  IconChevronLeftOutline14,
  IconChevronRightOutline14,
  IconPlusOutline16,
  IconSettingsOutline16,
  IconTrashOutline16,
} from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeRuntimeBinding } from '../native.ts'
import type { CollabDomainResult } from '../remote.ts'

/** Logical RPC channel served by the node half of this plugin. */
const RPC_CHANNEL = '/dsh-chaos'

/** Injected dependencies of {@link AgentsSection} (slot `inject`). */
export interface AgentsSectionInjected {
  /** Call one `/dsh-chaos` endpoint and unwrap both result layers. */
  call: <T>(endpoint: string, payload: unknown) => Promise<T>
  /** Host model catalog face (provider topology + model groups). */
  llm: IApiClient['llm']
}

/**
 * Props delivered by the slot outlet: the inject face spread flat, plus the
 * shell's owner share (`close`, unused by this page).
 */
export type AgentsSectionProps = Partial<AgentsSectionInjected> & { close?: () => void }

/**
 * Bind a `/dsh-chaos` caller to the Connection transport: transport RpcResult
 * first, then the CollabDomainResult business envelope (same unwrap as the
 * dock controller's private `call`).
 * @param rpc - Connection RPC caller.
 * @returns the injected `call` face.
 */
export function createChaosCall(rpc: ClientConnectionRpc): AgentsSectionInjected['call'] {
  return async function call<T>(endpoint: string, payload: unknown): Promise<T> {
    const carrier = await rpc.call(RPC_CHANNEL, endpoint, payload)
    if (!carrier.ok) throw new Error(carrier.error.message)
    const domain = carrier.value as CollabDomainResult<T>
    if (domain === null || typeof domain !== 'object' || typeof domain.ok !== 'boolean') {
      throw new Error('dsh-chaos Remote 返回了无效结果')
    }
    if (!domain.ok) throw new Error(`${domain.error.code}: ${domain.error.message}`)
    return domain.value
  }
}

/** Unwrap one host catalog RpcResponse (`res.result.ok ? value : throw`). */
async function callLlm<T>(request: Promise<{ result: { ok: true; value: T } | { ok: false; error: { message: string } } }>): Promise<T> {
  const res = await request
  if (!res.result.ok) throw new Error(res.result.error.message)
  return res.result.value
}

/** Human text of an action failure. */
function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

const styles: Record<string, CSSProperties> = {
  page: {
    display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 560,
    color: 'var(--dsw-alias-label-primary)',
  },
  pageTitle: {
    fontSize: 19, fontWeight: 600, lineHeight: '28px', margin: '4px 0 -4px 0',
    color: 'var(--dsw-alias-label-primary)',
  },
  intro: { margin: 0, color: 'var(--dsw-alias-label-tertiary)', fontSize: 14, lineHeight: '22px' },
  card: {
    border: '1px solid var(--dsw-alias-border-l2)', borderRadius: 12,
    padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 6,
  },
  cardHeader: { display: 'flex', alignItems: 'center', gap: 8 },
  icon: { display: 'inline-flex', flexShrink: 0, color: 'var(--dsw-alias-label-tertiary)' },
  dot: { width: 8, height: 8, borderRadius: '50%', flexShrink: 0, display: 'inline-block' },
  name: { fontWeight: 500, fontSize: 14, lineHeight: '22px', color: 'var(--dsw-alias-label-primary)' },
  statusLine: { margin: 0, fontSize: 12, lineHeight: '18px', color: 'var(--dsw-alias-label-tertiary)' },
  errorLine: { margin: 0, fontSize: 12, lineHeight: '18px', color: 'var(--dsw-alias-state-error-primary)' },
  mono: { fontFamily: 'ui-monospace,Menlo,monospace', fontSize: 12 },
  actions: { display: 'flex', gap: 8, marginTop: 4, alignItems: 'center', flexWrap: 'wrap' },
  pill: {
    boxSizing: 'border-box', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    gap: 6, height: 28, padding: '0 12px', borderRadius: 14,
    border: '1px solid var(--dsw-alias-border-l2)', background: 'transparent',
    color: 'var(--dsw-alias-label-primary)', font: 'inherit', fontSize: 12, lineHeight: '18px',
    cursor: 'pointer',
  },
  pillDanger: {
    boxSizing: 'border-box', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    gap: 6, height: 28, padding: '0 12px', borderRadius: 14,
    border: '1px solid var(--dsw-alias-state-error-primary)', background: 'transparent',
    color: 'var(--dsw-alias-state-error-primary)', font: 'inherit', fontSize: 12, lineHeight: '18px',
    cursor: 'pointer',
  },
  pillPrimary: {
    boxSizing: 'border-box', display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    gap: 6, height: 28, padding: '0 12px', borderRadius: 14, border: '1px solid transparent',
    background: 'var(--dsw-alias-state-business-primary)', color: '#fff',
    font: 'inherit', fontSize: 12, lineHeight: '18px', cursor: 'pointer',
  },
  field: { display: 'flex', flexDirection: 'column', gap: 4 },
  fieldLabel: { fontSize: 12, lineHeight: '18px', color: 'var(--dsw-alias-label-tertiary)' },
  input: {
    boxSizing: 'border-box', height: 32, border: '1px solid var(--dsw-alias-border-l2)',
    borderRadius: 8, padding: '0 10px', font: 'inherit', fontSize: 14, lineHeight: '22px',
    background: 'var(--dsw-alias-bg-layer-1)', color: 'var(--dsw-alias-label-primary)', width: '100%',
  },
  select: {
    boxSizing: 'border-box', height: 32, border: '1px solid var(--dsw-alias-border-l2)',
    borderRadius: 8, padding: '0 10px', font: 'inherit', fontSize: 14, lineHeight: '22px',
    background: 'var(--dsw-alias-bg-layer-1)', color: 'var(--dsw-alias-label-primary)', width: '100%',
    display: 'flex', alignItems: 'center', justifyContent: 'space-between', cursor: 'pointer',
  },
  grid: { display: 'grid', gridTemplateColumns: 'repeat(2,minmax(0,1fr))', gap: '10px 14px' },
  divider: { borderTop: '1px solid var(--dsw-alias-border-l2)', paddingTop: 8, marginTop: 2 },
  dropdown: {
    position: 'fixed', zIndex: 9999, border: '1px solid var(--dsw-alias-border-l2)',
    borderRadius: 8, boxShadow: '0 8px 28px rgba(0,0,0,0.22)', padding: 4,
    background: 'var(--dsw-alias-bg-layer-3)', maxHeight: 240, overflowY: 'auto',
  },
  ddGroup: {
    display: 'block', padding: '6px 10px 2px', fontSize: 12, lineHeight: '18px',
    color: 'var(--dsw-alias-label-dimmed)',
  },
  ddItem: {
    display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'center',
    padding: '6px 10px', borderRadius: 6, fontSize: 14, lineHeight: '22px',
    whiteSpace: 'nowrap', color: 'var(--dsw-alias-label-primary)', cursor: 'pointer',
  },
}

/** `$DSH_HOME/agents/<id>/` display form (the client never learns the real home). */
function agentDirLabel(agentId: string): string {
  return `$DSH_HOME/agents/${agentId}/`
}

/** Short data-dir form for list rows: `$DSH_HOME/agents/01a00fbb…/`. */
function agentDirShort(agentId: string): string {
  return `$DSH_HOME/agents/${agentId.slice(0, 8)}…/`
}

/** `YYYY-MM-DD HH:mm` in local time. */
function dateTimeLabel(ms: number): string {
  const d = new Date(ms)
  const pad = (n: number): string => String(n).padStart(2, '0')
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** `session <前4>…<后2> · gen N` for the detail grid. */
function bindingLabel(binding: NativeRuntimeBinding): string {
  const session = binding.sessionId
  const short = session.length > 6 ? `${session.slice(0, 4)}…${session.slice(-2)}` : session
  return `session ${short} · gen ${binding.generation}`
}

/** One dropdown entry of {@link CustomSelect}. */
interface SelectItem {
  value: string
  label: string
  /** Right-side secondary text (model count, guidance, …). */
  hint?: string
  /** Dimmed and not clickable (unconfigured providers). */
  disabled?: boolean
}

/** One labeled group of dropdown entries. */
interface SelectGroup {
  label?: string
  items: SelectItem[]
}

/**
 * Minimal grouped select (dsh-crew's CustomSelect, trimmed): a pill-styled
 * button opening a body-portal dropdown, closed by Escape or an outside
 * mousedown, flipping upwards near the viewport bottom.
 * @param props - current value, option groups, change handler.
 * @returns the select control.
 */
function CustomSelect(props: {
  value: string | undefined
  placeholder: string
  groups: readonly SelectGroup[]
  onChange: (value: string) => void
}) {
  const { value, placeholder, groups, onChange } = props
  const [open, setOpen] = useState<{ left: number; top: number; bottom: number; width: number; up: boolean } | null>(null)
  useEffect(() => {
    if (open === null) return
    const close = (): void => setOpen(null)
    // Capture at window + stopPropagation: the host closes its Settings
    // dialog from a document-level capture listener registered at app boot,
    // so only a window capture listener runs early enough to swallow Escape.
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape') return
      event.stopPropagation()
      close()
    }
    const timer = setTimeout(() => document.addEventListener('mousedown', close), 0)
    window.addEventListener('keydown', onKey, true)
    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [open])
  const items = groups.flatMap(group => group.items)
  const current = items.find(item => item.value === value)
  const [hovered, setHovered] = useState<string | undefined>(undefined)
  return (
    <>
      <button
        type="button"
        style={styles.select}
        onClick={(event) => {
          const rect = event.currentTarget.getBoundingClientRect()
          const up = window.innerHeight - rect.bottom < 240
          setOpen({
            left: rect.left,
            top: rect.bottom + 4,
            bottom: window.innerHeight - rect.top + 4,
            width: Math.max(rect.width, 160),
            up,
          })
        }}
      >
        <span style={current === undefined ? { color: 'var(--dsw-alias-label-dimmed)' } : undefined}>
          {current?.label ?? placeholder}
        </span>
        <span style={styles.icon}><IconChevronRightOutline14 size={14} /></span>
      </button>
      {open !== null && createPortal(
        <div
          style={{
            ...styles.dropdown,
            left: open.left,
            minWidth: open.width,
            ...(open.up ? { bottom: open.bottom } : { top: open.top }),
          }}
          onMouseDown={event => event.stopPropagation()}
        >
          {groups.map((group, index) => (
            <div key={group.label ?? index}>
              {group.label !== undefined && <span style={styles.ddGroup}>{group.label}</span>}
              {group.items.map((item) => {
                const selected = item.value === value
                const background = item.disabled === true
                  ? 'transparent'
                  : hovered === item.value || selected
                    ? 'var(--dsw-alias-interactive-bg-active)'
                    : 'transparent'
                return (
                  <div
                    key={item.value}
                    style={{
                      ...styles.ddItem,
                      background,
                      ...(item.disabled === true ? { opacity: 0.45, cursor: 'default' } : {}),
                    }}
                    onMouseEnter={() => setHovered(item.value)}
                    onMouseLeave={() => setHovered(undefined)}
                    onClick={() => {
                      if (item.disabled === true) return
                      onChange(item.value)
                      setOpen(null)
                    }}
                  >
                    <span>{item.label}</span>
                    {selected && (
                      <span style={{ color: 'var(--dsw-alias-state-success-primary)', fontSize: 12 }}>✓</span>
                    )}
                    {!selected && item.hint !== undefined && (
                      <span style={styles.statusLine}>{item.hint}</span>
                    )}
                  </div>
                )
              })}
            </div>
          ))}
        </div>,
        document.body,
      )}
    </>
  )
}

/** Status dot + label pair shared by list cards and the detail identity card. */
function StatusMark(props: { running: boolean }) {
  return (
    <>
      <span
        style={{
          ...styles.dot,
          background: props.running
            ? 'var(--dsw-alias-state-success-primary)'
            : 'var(--dsw-alias-label-dimmed)',
        }}
      />
      <span style={styles.statusLine}>{props.running ? '运行中' : '空闲'}</span>
    </>
  )
}

/** Back-to-list pill (chevron-left), shared by detail and create views. */
function BackButton(props: { onClick: () => void }) {
  return (
    <div>
      <button type="button" style={styles.pill} onClick={props.onClick}>
        <IconChevronLeftOutline14 size={14} /> 返回列表
      </button>
    </div>
  )
}

/** One loaded agent list row: actor plus its current runtime binding. */
interface AgentRow {
  actor: NativeActor
  binding?: NativeRuntimeBinding
}

/** `agent.profile` value shape (mirrors AgentProfile of the node half). */
interface AgentProfileValue {
  actor: NativeActor
  binding?: NativeRuntimeBinding
  workspacePath: string
}

/** `agent.create` value shape. */
interface AgentCreateValue {
  actor: NativeActor
  binding: NativeRuntimeBinding
  workspacePath: string
}

/** Page view state machine: list → drill-in detail, list → create form. */
type View = { kind: 'list' } | { kind: 'detail'; agentId: string } | { kind: 'create' }

/** Catalog lookup helper: display names and the reasoning flag by route/model id. */
interface Catalog {
  providers: ConfigurableProviderView[]
  groups: ModelProviderGroup[]
}

function providerLabel(catalog: Catalog, provider: string): string {
  return catalog.providers.find(entry => entry.provider === provider)?.displayName ?? provider
}

function modelOf(catalog: Catalog, provider: string, model: string): ModelCatalogModel | undefined {
  return catalog.groups.find(group => group.id === provider)?.models.find(entry => entry.id === model)
}

function modelLabel(catalog: Catalog, provider: string, model: string): string {
  return modelOf(catalog, provider, model)?.name ?? model
}

/** `Provider · 模型( · reasoning)` line for list cards and the detail identity card. */
function bindingSummary(catalog: Catalog, binding: NativeRuntimeBinding): string {
  const reasoning = modelOf(catalog, binding.provider, binding.model)?.reasoning !== undefined
  return `${providerLabel(catalog, binding.provider)} · ${modelLabel(catalog, binding.provider, binding.model)}${reasoning ? ' · reasoning' : ''}`
}

/**
 * The Agents settings page component.
 * @param props - the slot inject face ({@link AgentsSectionInjected}).
 * @returns the section body, or a notice while the RPC face is absent.
 */
export function AgentsSection(props: AgentsSectionProps) {
  const { call, llm } = props
  const [view, setView] = useState<View>({ kind: 'list' })
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | undefined>(undefined)
  const [rows, setRows] = useState<AgentRow[]>([])
  const [catalog, setCatalog] = useState<Catalog>({ providers: [], groups: [] })
  const mountedRef = useRef(true)

  /** Reload agents + bindings + the host provider/model catalog. */
  const reload = useCallback(async (): Promise<void> => {
    if (call === undefined || llm === undefined) return
    try {
      const [actors, bindings, providersValue, modelsValue] = await Promise.all([
        call<NativeActor[]>('actors', {}),
        call<NativeRuntimeBinding[]>('runtime.bindings', {}),
        callLlm<{ providers: ConfigurableProviderView[] }>(llm.providers({})),
        callLlm<{ groups: ModelProviderGroup[] }>(llm.models({})),
      ])
      if (!mountedRef.current) return
      const bindingByAgent = new Map(bindings.map(binding => [binding.agentId, binding]))
      const nextRows: AgentRow[] = actors
        .filter(actor => actor.kind === 'agent')
        .map((actor) => {
          const binding = bindingByAgent.get(actor.id)
          return binding === undefined ? { actor } : { actor, binding }
        })
      setRows(nextRows)
      setCatalog({ providers: providersValue.providers, groups: modelsValue.groups })
      setLoadError(undefined)
    } catch (error) {
      if (mountedRef.current) setLoadError(messageOf(error))
    } finally {
      if (mountedRef.current) setLoading(false)
    }
  }, [call, llm])

  // Initial load; re-load when the page becomes visible again (switching to
  // the「模型」section unmounts this page, so coming back remounts — the
  // visibility listener covers the settings-stays-open path).
  useEffect(() => {
    mountedRef.current = true
    void reload()
    const onVisible = (): void => {
      if (document.visibilityState === 'visible') void reload()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      mountedRef.current = false
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [reload])

  const backToList = useCallback((): void => {
    setView({ kind: 'list' })
    void reload()
  }, [reload])

  if (call === undefined || llm === undefined) {
    return <p style={styles.intro}>协作 Agents 服务不可用</p>
  }

  const activeProviders = catalog.providers.filter(provider => provider.active)

  const openCreate = (): void => setView({ kind: 'create' })

  let body
  if (loading) {
    body = <p style={styles.statusLine}>加载中…</p>
  } else if (view.kind === 'detail') {
    body = (
      <AgentDetail
        agentId={view.agentId}
        call={call}
        catalog={catalog}
        onBack={backToList}
        onDeleted={backToList}
      />
    )
  } else if (view.kind === 'create') {
    body = activeProviders.length === 0
      ? <EmptyGuide showBack={rows.length > 0} onBack={backToList} />
      : (
        <CreateForm
          call={call}
          catalog={catalog}
          activeProviders={activeProviders}
          onBack={backToList}
          onCreated={(agentId) => setView({ kind: 'detail', agentId })}
        />
      )
  } else if (rows.length === 0 && activeProviders.length === 0) {
    body = <EmptyGuide showBack={false} onBack={backToList} />
  } else {
    body = (
      <>
        {rows.map(row => (
          <AgentCard
            key={row.actor.id}
            row={row}
            catalog={catalog}
            onOpen={() => setView({ kind: 'detail', agentId: row.actor.id })}
          />
        ))}
        <div
          style={{ ...styles.card, borderStyle: 'dashed', alignItems: 'center', cursor: 'pointer' }}
          onClick={openCreate}
        >
          <div style={{ ...styles.cardHeader, color: 'var(--dsw-alias-label-secondary)' }}>
            <span style={styles.icon}><IconPlusOutline16 size={16} /></span>
            <span style={{ fontSize: 14, lineHeight: '22px' }}>新建 Agent</span>
          </div>
        </div>
      </>
    )
  }

  return (
    <div style={styles.page}>
      {view.kind === 'list' && (
        <>
          <div style={styles.pageTitle}>协作 Agents</div>
          <p style={styles.intro}>
            协作 Agent 以独立身份进频道回消息、领任务。点击卡片查看绑定与数据目录；新建前请确认已在「模型」里配置好 provider。
          </p>
        </>
      )}
      {loadError !== undefined && (
        <div style={styles.card}>
          <p style={styles.errorLine}>加载失败：{loadError}</p>
          <div style={styles.actions}>
            <button type="button" style={styles.pill} onClick={() => { void reload() }}>重试</button>
          </div>
        </div>
      )}
      {body}
    </div>
  )
}

/** One agent card in the list: icon + @handle + status dot + chevron, two summary lines. */
function AgentCard(props: { row: AgentRow; catalog: Catalog; onOpen: () => void }) {
  const { row, catalog, onOpen } = props
  return (
    <div style={{ ...styles.card, cursor: 'pointer' }} onClick={onOpen}>
      <div style={styles.cardHeader}>
        <span style={styles.icon}><IconAgentPresetOutline16 size={16} /></span>
        <span style={styles.name}>@{row.actor.handle}</span>
        <span style={{ flex: 1 }} />
        <StatusMark running={row.binding !== undefined} />
        <span style={styles.icon}><IconChevronRightOutline14 size={14} /></span>
      </div>
      <p style={styles.statusLine}>
        {row.binding === undefined ? '无 Runtime 绑定' : bindingSummary(catalog, row.binding)}
      </p>
      <p style={{ ...styles.statusLine, ...styles.mono }}>{agentDirShort(row.actor.id)}</p>
    </div>
  )
}

/** Drill-in detail: identity card, data-dir card, danger zone. */
function AgentDetail(props: {
  agentId: string
  call: AgentsSectionInjected['call']
  catalog: Catalog
  onBack: () => void
  onDeleted: () => void
}) {
  const { agentId, call, catalog, onBack, onDeleted } = props
  const [profile, setProfile] = useState<AgentProfileValue | undefined>(undefined)
  const [error, setError] = useState<string | undefined>(undefined)
  const [deleting, setDeleting] = useState(false)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => { mountedRef.current = false }
  }, [])

  useEffect(() => {
    let stale = false
    setProfile(undefined)
    setError(undefined)
    call<AgentProfileValue>('agent.profile', { agentId })
      .then((value) => { if (!stale) setProfile(value) })
      .catch((loadError: unknown) => { if (!stale) setError(messageOf(loadError)) })
    return () => { stale = true }
  }, [call, agentId])

  const remove = async (): Promise<void> => {
    if (profile === undefined || deleting) return
    if (!window.confirm(`确认删除 Agent @${profile.actor.handle}？将从所有频道移除并销毁其运行时绑定，不可恢复。`)) return
    setDeleting(true)
    setError(undefined)
    try {
      await call('agent.delete', { agentId })
      if (mountedRef.current) onDeleted()
    } catch (deleteError) {
      if (mountedRef.current) {
        setError(messageOf(deleteError))
        setDeleting(false)
      }
    }
  }

  return (
    <>
      <BackButton onClick={onBack} />
      <div style={styles.card}>
        {profile === undefined && error === undefined && <p style={styles.statusLine}>加载中…</p>}
        {profile !== undefined && (
          <>
            <div style={styles.cardHeader}>
              <span style={styles.icon}><IconAgentPresetOutline16 size={16} /></span>
              <span style={styles.name}>@{profile.actor.handle}</span>
              <span style={{ flex: 1 }} />
              <StatusMark running={profile.binding !== undefined} />
            </div>
            <p style={styles.statusLine}>
              {profile.binding === undefined ? '无 Runtime 绑定' : bindingSummary(catalog, profile.binding)}
            </p>
            <div style={{ ...styles.grid, ...styles.divider }}>
              <label style={styles.field}>
                <span style={styles.fieldLabel}>Runtime 绑定</span>
                <span style={{ fontSize: 14, lineHeight: '22px', ...styles.mono }}>
                  {profile.binding === undefined ? '无' : bindingLabel(profile.binding)}
                </span>
              </label>
              <label style={styles.field}>
                <span style={styles.fieldLabel}>创建时间</span>
                <span style={{ fontSize: 14, lineHeight: '22px' }}>{dateTimeLabel(profile.actor.createdAtMs)}</span>
              </label>
            </div>
          </>
        )}
      </div>
      {profile !== undefined && (
        <div style={styles.card}>
          <div style={styles.cardHeader}><span style={styles.name}>数据目录</span></div>
          <p style={styles.statusLine}>创建时自动分配，无需选择</p>
          <p style={{ ...styles.statusLine, ...styles.mono, color: 'var(--dsw-alias-label-secondary)' }}>
            {profile.workspacePath}
          </p>
        </div>
      )}
      {profile !== undefined && (
        <div style={styles.card}>
          <div style={styles.cardHeader}>
            <span style={{ ...styles.name, color: 'var(--dsw-alias-state-error-primary)' }}>危险操作</span>
          </div>
          <p style={styles.statusLine}>从所有频道移除并销毁其运行时绑定，不可恢复</p>
          <div style={styles.actions}>
            <button
              type="button"
              style={{ ...styles.pillDanger, ...(deleting ? { opacity: 0.5, cursor: 'default' } : {}) }}
              disabled={deleting}
              onClick={() => { void remove() }}
            >
              <IconTrashOutline16 size={16} /> {deleting ? '删除中…' : '删除 Agent'}
            </button>
          </div>
        </div>
      )}
      {error !== undefined && <p style={styles.errorLine}>{error}</p>}
    </>
  )
}

/** Create form: name + grouped provider select + linked model select + read-only data dir. */
function CreateForm(props: {
  call: AgentsSectionInjected['call']
  catalog: Catalog
  activeProviders: ConfigurableProviderView[]
  onBack: () => void
  onCreated: (agentId: string) => void
}) {
  const { call, catalog, activeProviders, onBack, onCreated } = props
  const [name, setName] = useState('')
  const [provider, setProvider] = useState<string | undefined>(activeProviders[0]?.provider)
  const [model, setModel] = useState<string | undefined>(undefined)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | undefined>(undefined)

  const modelGroup = catalog.groups.find(group => group.id === provider)
  const models = modelGroup?.models ?? []
  const effectiveModel = models.some(entry => entry.id === model) ? model : models[0]?.id

  const providerGroups: SelectGroup[] = [
    {
      label: '已配置',
      items: activeProviders.map((entry) => {
        const count = catalog.groups.find(group => group.id === entry.provider)?.models.length
        return {
          value: entry.provider,
          label: entry.displayName,
          ...(count === undefined ? {} : { hint: `${String(count)} 个模型` }),
        }
      }),
    },
    {
      label: '未配置',
      items: catalog.providers
        .filter(entry => !entry.active)
        .map(entry => ({ value: entry.provider, label: entry.displayName, hint: '去「模型」设置', disabled: true })),
    },
  ].filter(group => group.items.length > 0)

  const submit = async (): Promise<void> => {
    if (submitting) return
    const trimmed = name.trim()
    if (trimmed === '' || provider === undefined || effectiveModel === undefined) {
      setError('请填写名称并选择 Provider 和模型')
      return
    }
    setSubmitting(true)
    setError(undefined)
    try {
      const created = await call<AgentCreateValue>('agent.create', {
        name: trimmed,
        provider,
        model: effectiveModel,
      })
      onCreated(created.actor.id)
    } catch (createError) {
      // Keep every input on failure so the user can adjust and retry.
      setError(messageOf(createError))
      setSubmitting(false)
    }
  }

  return (
    <>
      <BackButton onClick={onBack} />
      <div style={styles.card}>
        <div style={styles.cardHeader}>
          <span style={styles.icon}><IconPlusOutline16 size={16} /></span>
          <span style={styles.name}>新建 Agent</span>
        </div>
        <p style={styles.statusLine}>模型来自宿主已配置的 provider；未配置的请先去「模型」设置</p>
        <div style={{ ...styles.grid, ...styles.divider }}>
          <label style={styles.field}>
            <span style={styles.fieldLabel}>名称（@身份，唯一）</span>
            <input
              style={styles.input}
              value={name}
              onChange={event => setName(event.target.value)}
              placeholder="reviewer"
            />
          </label>
          <label style={styles.field}>
            <span style={styles.fieldLabel}>Provider</span>
            <CustomSelect
              value={provider}
              placeholder="选择 Provider"
              groups={providerGroups}
              onChange={(value) => {
                setProvider(value)
                setModel(undefined)
              }}
            />
          </label>
          <label style={styles.field}>
            <span style={styles.fieldLabel}>模型</span>
            <CustomSelect
              value={effectiveModel}
              placeholder={models.length === 0 ? '该 Provider 暂无可用模型' : '选择模型'}
              groups={[{
                items: models.map(entry => ({ value: entry.id, label: entry.name })),
              }]}
              onChange={setModel}
            />
          </label>
          <label style={styles.field}>
            <span style={styles.fieldLabel}>数据目录</span>
            <span
              style={{
                ...styles.input,
                display: 'flex', alignItems: 'center',
                ...styles.mono, fontSize: 12, color: 'var(--dsw-alias-label-dimmed)',
              }}
            >
              {agentDirLabel('<自动>')}
            </span>
          </label>
        </div>
        <div style={{ ...styles.actions, ...styles.divider }}>
          <button
            type="button"
            style={{ ...styles.pillPrimary, ...(submitting ? { opacity: 0.5, cursor: 'default' } : {}) }}
            disabled={submitting}
            onClick={() => { void submit() }}
          >
            {submitting ? '创建中…' : '创建 Agent'}
          </button>
          <span style={styles.statusLine}>创建后跳到详情页，可立即去频道邀请</span>
        </div>
        {error !== undefined && <p style={styles.errorLine}>{error}</p>}
      </div>
    </>
  )
}

/** Centered guide card for "no configured provider" (create form hidden). */
function EmptyGuide(props: { showBack: boolean; onBack: () => void }) {
  return (
    <>
      {props.showBack && <BackButton onClick={props.onBack} />}
      <div style={{ ...styles.card, alignItems: 'center', textAlign: 'center', padding: '28px 14px' }}>
        <span style={{ ...styles.icon, color: 'var(--dsw-alias-label-dimmed)' }}>
          <IconSettingsOutline16 size={28} />
        </span>
        <span style={styles.name}>还没有可用的模型服务</span>
        <p style={{ ...styles.statusLine, maxWidth: 380 }}>
          创建 Agent 前，需要先在「模型」设置里配置至少一个 provider（一次性）。
        </p>
        <p style={{ ...styles.statusLine, maxWidth: 380 }}>
          在左侧「模型」中配置后，回到这里会自动刷新。
        </p>
      </div>
    </>
  )
}
