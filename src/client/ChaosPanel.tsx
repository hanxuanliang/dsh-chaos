import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type FormEvent, type ReactNode } from 'react'
import type { HostObservable, InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type { ChaosClientState } from './controller.ts'
import type { NativeActor } from '../native.ts'
import { ThreadPanel } from './ThreadPanel.tsx'
import { claimWorkbenchDock, WORKBENCH_DEFAULT_WIDTH, type WorkbenchDockLease } from './workbench-dock.ts'
import css from './ChaosPanel.module.css'

export interface ChaosPanelInjected {
  hooks: { chaos: HostObservable<ChaosClientState> }
  ensure: () => Promise<void>
  refresh: () => Promise<void>
  toggleRail: () => void
  openRail: () => void
  setRailTab: (tab: ChaosClientState['railTab']) => void
  openWorkbench: () => void
  closeWorkbench: () => void
  setAsTask: (asTask: boolean) => void
  openDesk: (agentId?: string) => void
  closeSurface: () => void
  clearTarget: () => void
  selectTarget: (targetId: string) => Promise<void>
  createChannel: (name: string) => Promise<void>
  createAgent: (name: string) => Promise<void>
  openAgent: (agentId: string) => void
  createDirect: (peerId: string) => Promise<void>
  addMember: (targetId: string, memberId: string) => Promise<void>
  createThread: (rootMessageId: string) => Promise<void>
  followThread: (threadTargetId: string) => Promise<void>
  unfollowThread: (threadTargetId: string) => Promise<void>
  openThreadPanel: (threadTargetId: string) => Promise<void>
  closeThreadPanel: () => void
  sendToThread: (text: string) => Promise<void>
  send: (text: string) => Promise<void>
  sendAsTask: (text: string) => Promise<void>
  createTask: (messageId: string) => Promise<void>
}

export type ChaosPanelProps = PropsRuntime<'shell.overlay'> & InjectFace<ChaosPanelInjected>
export type ChaosDockProps = PropsRuntime<'conversation.input.dock'> & InjectFace<ChaosPanelInjected>

/**
 * Modal dialog keyboard contract: Escape cancels, and Tab/Shift+Tab cycle
 * inside the dialog instead of escaping into the host page.
 */
function useDialogKeys(
  containerRef: { readonly current: HTMLElement | null },
  onCancel: () => void,
): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        onCancel()
        return
      }
      if (event.key !== 'Tab') return
      const root = containerRef.current
      if (root === null) return
      const focusables = root.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      )
      if (focusables.length === 0) return
      const first = focusables.item(0)
      const last = focusables.item(focusables.length - 1)
      const active = document.activeElement
      if (event.shiftKey && (active === first || !root.contains(active))) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && (active === last || !root.contains(active))) {
        event.preventDefault()
        first.focus()
      }
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => { window.removeEventListener('keydown', onKeyDown, true) }
  }, [containerRef, onCancel])
}

const CloseIcon = (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden>
    <path d="M18 6 6 18" />
    <path d="m6 6 12 12" />
  </svg>
)

const PanelIcon = (
  <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden>
    <rect x="2.5" y="2.5" width="11" height="11" rx="1.5" />
    <path d="M10.5 3v10" />
  </svg>
)

const RAIL_WIDTH_PX = 276

function RoomWorkbench({
  title,
  railOpen,
  onClose,
}: {
  title: string
  railOpen: boolean
  onClose: () => void
}): ReactNode {
  const leaseRef = useRef<WorkbenchDockLease>()
  const ownerId = 'dsh-chaos-workbench'
  const dockWidth = WORKBENCH_DEFAULT_WIDTH + (railOpen ? RAIL_WIDTH_PX : 0)

  useLayoutEffect(() => {
    const root = document.getElementById('root')
    if (root === null) return
    const computed = Number.parseFloat(window.getComputedStyle(root).marginRight)
    const lease = claimWorkbenchDock(root, ownerId, dockWidth, computed)
    if (lease === undefined) return
    leaseRef.current = lease
    return () => {
      if (leaseRef.current === lease) leaseRef.current = undefined
      lease.release()
    }
  }, [])

  useLayoutEffect(() => {
    leaseRef.current?.update(dockWidth)
  }, [dockWidth])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => { window.removeEventListener('keydown', onKeyDown) }
  }, [onClose])

  return (
    <aside
      className={css.workbench}
      aria-label={title}
      style={{ right: railOpen ? `${String(RAIL_WIDTH_PX)}px` : '0' }}
    >
      <header className={css.workbenchHead}>
        <strong>{title}</strong>
        <button type="button" className={css.iconButton} aria-label="关闭房间" onClick={onClose}>
          {CloseIcon}
        </button>
      </header>
      <div className={css.workbenchBody}>
        <p className={css.empty}>房间时间线下一刀再铺。</p>
      </div>
    </aside>
  )
}

/** Centered name-only create dialog. Path and Session stay off-screen. */
function NamedCreateDialog({
  title,
  fieldLabel,
  pending,
  error,
  onCancel,
  onCreate,
}: {
  title: string
  fieldLabel: string
  pending: boolean
  error: string | null
  onCancel: () => void
  onCreate: (name: string) => Promise<boolean>
}) {
  const [name, setName] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const dialogRef = useRef<HTMLFormElement>(null)

  useEffect(() => { inputRef.current?.focus() }, [])
  useDialogKeys(dialogRef, onCancel)

  const submit = (event: FormEvent): void => {
    event.preventDefault()
    const trimmed = name.trim()
    if (trimmed === '' || pending) return
    void onCreate(trimmed).then(created => {
      if (created) setName('')
      else inputRef.current?.focus()
    })
  }

  return (
    <div
      className={css.dialogOverlay}
      onMouseDown={event => {
        if (event.target === event.currentTarget) onCancel()
      }}
    >
      <form
        ref={dialogRef}
        className={css.dialog}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onSubmit={submit}
      >
        <strong className={css.dialogTitle}>{title}</strong>
        <input
          ref={inputRef}
          value={name}
          onChange={event => { setName(event.target.value) }}
          placeholder={fieldLabel}
          aria-label={fieldLabel}
        />
        {error !== null && <div className={css.dialogError} role="alert">{error}</div>}
        <div className={css.dialogActions}>
          <button type="button" className={css.secondaryButton} onClick={onCancel}>取消</button>
          <button className={css.primaryButton} disabled={pending || name.trim() === ''}>
            {pending ? '创建中…' : '创建'}
          </button>
        </div>
      </form>
    </div>
  )
}

function useChaosStore(store: HostObservable<ChaosClientState>): ChaosClientState {
  return useSyncExternalStore(
    listener => store.subscribe(listener),
    () => store.getSnapshot(),
    () => store.getSnapshot(),
  )
}

function chaosStateOf(props: ChaosPanelProps | ChaosPanelInjected): ChaosClientState {
  return 'useChaos' in props ? props.useChaos(value => value) : useChaosStore(props.hooks.chaos)
}

function useRosterActions(
  props: Pick<ChaosPanelInjected, 'createChannel' | 'createAgent'>,
  state: ChaosClientState,
) {
  const [createKind, setCreateKind] = useState<'channel' | 'agent' | null>(null)
  const [createPending, setCreatePending] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const plusRef = useRef<HTMLButtonElement>(null)
  const agentPlusRef = useRef<HTMLButtonElement>(null)
  const selected = state.targets.find(target => target.id === state.selectedTargetId)

  const submitCreate = async (name: string): Promise<boolean> => {
    if (createPending || createKind === null) return false
    setCreatePending(true)
    setCreateError(null)
    try {
      if (createKind === 'channel') await props.createChannel(name)
      else await props.createAgent(name)
      const kind = createKind
      setCreateKind(null)
      if (kind === 'channel') plusRef.current?.focus()
      else agentPlusRef.current?.focus()
      return true
    } catch (error) {
      setCreateError(error instanceof Error ? error.message : String(error))
      return false
    } finally {
      setCreatePending(false)
    }
  }

  return {
    createKind,
    setCreateKind,
    createPending,
    createError,
    plusRef,
    agentPlusRef,
    selected,
    submitCreate,
  }
}

function ChannelList({
  state,
  plusRef,
  onCreate,
  onSelect,
}: {
  state: ChaosClientState
  plusRef: { current: HTMLButtonElement | null }
  onCreate: () => void
  onSelect: (targetId: string) => void
}): ReactNode {
  const channels = state.targets.filter(target => target.kind === 'channel')
  return (
    <div className={css.rosterPane}>
      <div className={css.membersHead}>
        <h3>CHANNELS</h3>
        <button
          ref={plusRef}
          type="button"
          className={css.plusButton}
          aria-label="新建 Channel"
          title="新建 Channel"
          onClick={onCreate}
        >
          ＋
        </button>
      </div>
      <nav className={css.targets} aria-label="Channels">
        {channels.map(channel => (
          <button
            type="button"
            key={channel.id}
            className={css.targetButton}
            data-selected={channel.id === state.selectedTargetId || undefined}
            aria-label={`选择 Channel ${channel.name}`}
            onClick={() => { onSelect(channel.id) }}
          >
            <span className={css.targetGlyph} aria-hidden>#</span>
            <span>{channel.name}</span>
          </button>
        ))}
        {channels.length === 0 && <p className={css.empty}>用 ＋ 新建一个 Channel。</p>}
      </nav>
    </div>
  )
}

function AgentList({
  state,
  plusRef,
  onCreate,
  onOpen,
}: {
  state: ChaosClientState
  plusRef: { current: HTMLButtonElement | null }
  onCreate: () => void
  onOpen: (agentId: string) => void
}): ReactNode {
  const agents = state.actors.filter(actor => actor.kind === 'agent')
  const hostSessionBound = (actor: NativeActor): boolean =>
    state.bindings.some(binding => binding.agentId === actor.id && binding.sessionId === state.hostSessionId)
  return (
    <div className={css.rosterPane}>
      <div className={css.membersHead}>
        <h3>AGENTS</h3>
        <button
          ref={plusRef}
          type="button"
          className={css.plusButton}
          aria-label="新建 Agent"
          title="新建 Agent"
          onClick={onCreate}
        >
          ＋
        </button>
      </div>
      <nav className={css.targets} aria-label="Agents">
        {agents.map(actor => (
          <button
            type="button"
            key={actor.id}
            className={css.targetButton}
            data-selected={state.selectedAgentId === actor.id || hostSessionBound(actor) || undefined}
            aria-label={`打开 Agent ${actor.displayName}`}
            onClick={() => { onOpen(actor.id) }}
          >
            <span>{actor.displayName}</span>
            {hostSessionBound(actor) && <span className={css.badge}>当前</span>}
          </button>
        ))}
        {agents.length === 0 && <p className={css.empty}>用 ＋ 只填一个名字。</p>}
      </nav>
    </div>
  )
}

export function ChannelsPage(props: ChaosPanelProps | ChaosPanelInjected): ReactNode {
  const state = chaosStateOf(props)
  const roster = useRosterActions(props, state)
  useEffect(() => { void props.ensure() }, [props.ensure])
  return (
    <>
      <ChannelList
        state={state}
        plusRef={roster.plusRef}
        onCreate={() => {
          roster.setCreateKind('channel')
        }}
        onSelect={targetId => { void props.selectTarget(targetId) }}
      />
      {roster.createKind === 'channel' && (
        <NamedCreateDialog
          title="新建 Channel"
          fieldLabel="Channel 名称"
          pending={roster.createPending}
          error={roster.createError}
          onCancel={() => {
            roster.setCreateKind(null)
            roster.plusRef.current?.focus()
          }}
          onCreate={roster.submitCreate}
        />
      )}
    </>
  )
}

export function AgentsPage(props: ChaosPanelProps | ChaosPanelInjected): ReactNode {
  const state = chaosStateOf(props)
  const roster = useRosterActions(props, state)
  useEffect(() => { void props.ensure() }, [props.ensure])
  return (
    <>
      <AgentList
        state={state}
        plusRef={roster.agentPlusRef}
        onCreate={() => { roster.setCreateKind('agent') }}
        onOpen={props.openAgent}
      />
      {roster.createKind === 'agent' && (
        <NamedCreateDialog
          title="新建 Agent"
          fieldLabel="Agent 名称"
          pending={roster.createPending}
          error={roster.createError}
          onCancel={() => {
            roster.setCreateKind(null)
            roster.agentPlusRef.current?.focus()
          }}
          onCreate={roster.submitCreate}
        />
      )}
    </>
  )
}

export function ThreadPage(props: ChaosPanelProps | ChaosPanelInjected): ReactNode {
  const state = chaosStateOf(props)
  const names = useMemo(
    () => new Map(state.actors.map(actor => [actor.id, actor.displayName])),
    [state.actors],
  )
  const kinds = useMemo(
    () => new Map(state.actors.map(actor => [actor.id, actor.kind])),
    [state.actors],
  )
  const panelThread = state.targets.find(target => target.id === state.threadPanelId)
  const panelParent = panelThread === undefined
    ? undefined
    : state.targets.find(target => target.id === panelThread.parentTargetId)
  useEffect(() => { void props.ensure() }, [props.ensure])
  if (panelThread === undefined) {
    return <p className={css.empty}>点一条回复，这里打开 Thread。</p>
  }
  return (
    <ThreadPanel
      thread={panelThread}
      parentName={panelParent?.name ?? 'Thread'}
      messages={state.threadPanelMessages}
      names={names}
      kinds={kinds}
      onClose={() => {
        props.closeThreadPanel()
        props.setRailTab('channels')
      }}
    />
  )
}

const HERO_CHIP_GAP = 2

function paintedRight(root: Element): number | null {
  let right: number | null = null
  const visit = (node: Element): void => {
    if (node !== root) {
      const rect = node.getBoundingClientRect()
      if (rect.width > 0 && rect.height > 0) {
        right = right === null ? rect.right : Math.max(right, rect.right)
      }
    }
    for (const child of Array.from(node.children)) visit(child)
  }
  visit(root)
  return right
}

/** Official composer chips: sit on the Standard mode row, same 28px pill. */
export function ChaosDock({
  useChaos,
  ensure,
  setAsTask,
  clearTarget,
  closeThreadPanel,
}: ChaosDockProps) {
  const state = useChaos(value => value)
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const thread = state.railTab === 'thread'
    ? state.targets.find(target => target.id === state.threadPanelId)
    : undefined
  const parent = thread === undefined
    ? undefined
    : state.targets.find(target => target.id === thread.parentTargetId)
  const [heroPlacement, setHeroPlacement] = useState<{ left: number; top: number } | null>(null)
  const anchorRef = useRef<HTMLDivElement>(null)

  useEffect(() => { void ensure() }, [ensure])
  useLayoutEffect(() => {
    const anchor = anchorRef.current
    const outlet = anchor?.parentElement ?? null
    const stack = outlet?.parentElement ?? null
    const heroRow = outlet?.previousElementSibling ?? null
    if (anchor === null || outlet === null || stack === null || heroRow === null) return
    const measure = (): void => {
      const stackRect = stack.getBoundingClientRect()
      const rowRect = heroRow.getBoundingClientRect()
      const anchorRect = anchor.getBoundingClientRect()
      if (stackRect.width <= 0 || rowRect.width <= 0 || anchorRect.width <= 0) return
      const right = paintedRight(heroRow)
      if (right === null) return
      const left = Math.max(0, right - stackRect.left + HERO_CHIP_GAP)
      const top = Math.max(0, rowRect.top - stackRect.top + (rowRect.height - anchorRect.height) / 2)
      setHeroPlacement(previous => {
        if (previous !== null && Math.abs(previous.left - left) < 0.5 && Math.abs(previous.top - top) < 0.5) {
          return previous
        }
        return { left, top }
      })
    }
    measure()
    const observer = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(measure)
    for (const target of [anchor, outlet, stack, heroRow]) observer?.observe(target)
    window.addEventListener('resize', measure)
    return () => {
      observer?.disconnect()
      window.removeEventListener('resize', measure)
    }
  }, [selected?.id, thread?.id, state.asTask])

  if (thread === undefined && selected === undefined) return null

  return (
    <div
      ref={anchorRef}
      className={css.heroAnchor}
      data-chaos-dock={thread === undefined ? 'channel' : 'thread'}
      style={heroPlacement === null ? undefined : { left: `${String(heroPlacement.left)}px`, top: `${String(heroPlacement.top)}px` }}
    >
      {thread !== undefined ? (
        <button
          type="button"
          className={css.heroChip}
          aria-label="Clear thread target"
          onClick={closeThreadPanel}
        >
          <span>#{parent?.name ?? 'thread'}</span>
        </button>
      ) : (
        <>
          <button
            type="button"
            className={css.heroChip}
            aria-label={`Clear #${selected?.name ?? 'channel'}`}
            onClick={clearTarget}
          >
            <span>#{selected?.name}</span>
          </button>
          {selected?.kind === 'channel' && (
            <button
              type="button"
              role="checkbox"
              aria-checked={state.asTask}
              aria-label="As Task"
              className={css.heroChip}
              data-checked={state.asTask || undefined}
              onClick={() => { setAsTask(!state.asTask) }}
            >
              As Task
            </button>
          )}
        </>
      )}
    </div>
  )
}

/**
 * Fallback when better-sidebar is not installed: same seating as sidecar —
 * a top-right toggle opens a right workbench. Center stays official DSH.
 */
export function ChaosPanel(props: ChaosPanelProps) {
  const {
    useChaos,
    ensure,
    toggleRail,
    setRailTab,
    closeSurface,
    closeWorkbench,
    clearTarget,
  } = props
  const state = useChaos(value => value)
  const open = state.surface === 'rail'
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const workbenchOpen = state.workbench === 'open' && selected !== undefined

  useEffect(() => { void ensure() }, [ensure])

  const tab = state.threadPanelId !== undefined && state.railTab === 'thread'
    ? 'thread'
    : state.railTab === 'agents' ? 'agents' : 'channels'

  return (
    <>
      <div className={css.toggleCluster}>
        <button
          type="button"
          className={css.toggleButton}
          aria-label={open ? '收起协作面板' : '打开协作面板'}
          aria-expanded={open}
          onClick={toggleRail}
        >
          {PanelIcon}
        </button>
      </div>
      <aside
        className={`${css.panel} ${open ? '' : css.panelHidden}`}
        aria-label="协作面板"
        data-stream={state.stream}
      >
        <div className={css.tabBar}>
          <button
            type="button"
            className={css.tab}
            data-active={tab === 'channels' || undefined}
            onClick={() => { setRailTab('channels') }}
          >
            Channels
          </button>
          <button
            type="button"
            className={css.tab}
            data-active={tab === 'agents' || undefined}
            onClick={() => { setRailTab('agents') }}
          >
            Agents
          </button>
          {state.threadPanelId !== undefined && (
            <button
              type="button"
              className={css.tab}
              data-active={tab === 'thread' || undefined}
              onClick={() => { setRailTab('thread') }}
            >
              Thread
            </button>
          )}
          <button type="button" className={css.iconButton} aria-label="关闭协作面板" onClick={closeSurface}>
            {CloseIcon}
          </button>
        </div>
        <div className={css.panelBody}>
          {tab === 'agents'
            ? <AgentsPage {...props} />
            : tab === 'thread'
              ? <ThreadPage {...props} />
              : <ChannelsPage {...props} />}
        </div>
      </aside>
      {workbenchOpen && selected !== undefined && (
        <RoomWorkbench
          title={`#${selected.name}`}
          railOpen={open}
          onClose={() => {
            closeWorkbench()
            clearTarget()
          }}
        />
      )}
    </>
  )
}
