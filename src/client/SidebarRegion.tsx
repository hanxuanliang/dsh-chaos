import { useEffect, useMemo, useState } from 'react'
import type { InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type { SessionId } from '@deepseek-ai/dsh-client-runtime/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import type { NativeActivityInboxItem } from '../native.ts'
import type { ChaosClientState } from './controller.ts'
import {
  TASK_STATUS_TEXT,
  timeLabel,
  useChaos,
  type ChaosInjected,
} from './Workbench.tsx'
import css from './SidebarRegion.module.css'

type RegionProps = PropsRuntime<'sidebar.workspaces'> & InjectFace<ChaosInjected>

type RegionTab = 'sessions' | 'activity'

function ChatGlyph(): React.JSX.Element {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M2.5 3.5a1 1 0 0 1 1-1h9a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H6l-3.5 3v-10Z"
        stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round"
      />
    </svg>
  )
}

function BellGlyph(): React.JSX.Element {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M8 2a4 4 0 0 0-4 4v2.4L2.8 10h10.4L12 8.4V6a4 4 0 0 0-4-4ZM6.5 12a1.5 1.5 0 0 0 3 0"
        stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"
      />
    </svg>
  )
}

function relTime(ms: number): string {
  const diff = Date.now() - ms
  if (diff < 60_000) return '刚刚'
  const minutes = Math.floor(diff / 60_000)
  if (minutes < 60) return `${String(minutes)} 分钟前`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${String(hours)} 小时前`
  return timeLabel(ms)
}

/**
 * Sidebar browsing region: replaces the official Workspace browser (priority
 * shadow) with two tabs — the host Session list and the chaos Activity inbox.
 */
export function SidebarRegion(props: RegionProps): React.JSX.Element {
  const [tab, setTab] = useState<RegionTab>('sessions')
  const state = useChaos(props)
  const activeCount = state.inbox.activeCount

  if (!props.wide) {
    return (
      <div className={css.rail}>
        <button
          type="button"
          className={css.railButton}
          aria-label="会话"
          title="会话"
          onClick={() => {
            setTab('sessions')
            props.expandSidebar()
          }}
        >
          <ChatGlyph />
        </button>
        <button
          type="button"
          className={css.railButton}
          aria-label="Activity"
          title="Activity"
          onClick={() => {
            setTab('activity')
            props.expandSidebar()
          }}
        >
          <BellGlyph />
        </button>
      </div>
    )
  }

  return (
    <section className={css.region} aria-label="会话与协作动态">
      <div className={css.tabs} role="tablist" aria-label="侧栏视图">
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'sessions'}
          className={css.tab}
          data-active={tab === 'sessions' || undefined}
          onClick={() => { setTab('sessions') }}
        >
          <ChatGlyph />
          <span>会话</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === 'activity'}
          className={css.tab}
          data-active={tab === 'activity' || undefined}
          onClick={() => { setTab('activity') }}
        >
          <BellGlyph />
          <span>Activity</span>
          {activeCount !== '0'
            ? <span className={css.tabBadge}>{activeCount}</span>
            : null}
        </button>
      </div>
      {tab === 'sessions'
        ? <SessionsPane {...props} />
        : <ActivityPane {...props} state={state} />}
    </section>
  )
}

interface SessionRowModel {
  id: SessionId
  title: string
  status: 'warn' | 'run' | 'done' | 'idle'
}

function SessionsPane(props: RegionProps): React.JSX.Element {
  const ids = props.useSessions(value => value.ids)
  const byId = props.useSessions(value => value.byId)
  const current = props.useSessions(value => value.current)
  const workspaces = props.useWorkspaces(value => value.items)
  const archived = props.useWorkspaces(value => value.archivedSessionIds)
  const [query, setQuery] = useState('')

  const archivedSet = useMemo(() => new Set<SessionId>(archived), [archived])
  const q = query.trim().toLowerCase()

  const { groups, rest } = useMemo(() => {
    const visible = (id: SessionId): boolean => {
      const session = byId[id]
      if (session === undefined || archivedSet.has(id)) return false
      if (session.origin === 'subagent') return false
      if (session.blank && session.id !== current) return false
      if (q !== '' && !session.displayTitle.toLowerCase().includes(q)) return false
      return true
    }
    const grouped = workspaces
      .map(workspace => ({
        id: workspace.workspaceId,
        title: workspace.title,
        sessions: workspace.sessionIds.filter(visible),
      }))
      .filter(group => group.sessions.length > 0)
    const accounted = new Set(workspaces.flatMap(workspace => workspace.sessionIds))
    return { groups: grouped, rest: ids.filter(id => !accounted.has(id) && visible(id)) }
  }, [ids, byId, current, workspaces, archivedSet, q])

  const rowModel = (id: SessionId): SessionRowModel => {
    const session = byId[id]
    const status: SessionRowModel['status'] = session?.pendingInteraction !== undefined
      ? 'warn'
      : session?.running === true
        ? 'run'
        : session?.completed === true
          ? 'done'
          : 'idle'
    return { id, title: session?.displayTitle ?? id, status }
  }

  const renderRow = (id: SessionId): React.JSX.Element => {
    const row = rowModel(id)
    return (
      <button
        key={row.id}
        type="button"
        className={css.sessionRow}
        data-active={row.id === current || undefined}
        title={row.title}
        onClick={() => { props.openSession(row.id) }}
      >
        <span className={css.sessionDot} data-status={row.status} />
        <span className={css.sessionTitle}>{row.title}</span>
      </button>
    )
  }

  const empty = groups.length === 0 && rest.length === 0

  return (
    <div className={css.pane}>
      <div className={css.searchWrap}>
        <input
          className={css.search}
          value={query}
          placeholder="搜索会话"
          aria-label="搜索会话"
          onChange={event => { setQuery(event.target.value) }}
        />
      </div>
      <div className={css.list}>
        {groups.map(group => (
          <div key={group.id} className={css.group}>
            <div className={css.groupTitle}>{group.title}</div>
            {group.sessions.map(renderRow)}
          </div>
        ))}
        {rest.length > 0
          ? (
            <div className={css.group}>
              {groups.length > 0 ? <div className={css.groupTitle}>未分组</div> : null}
              {rest.map(renderRow)}
            </div>
          )
          : null}
        {empty
          ? (
            <div className={css.empty}>
              {q !== '' ? '没有匹配的会话' : '还没有会话，点上方「新会话」开始。'}
            </div>
          )
          : null}
      </div>
    </div>
  )
}

function ActivityCard(props: RegionProps & {
  state: ChaosClientState
  item: NativeActivityInboxItem
  onError: (message: string) => void
}): React.JSX.Element {
  const { item } = props
  const [donePending, setDonePending] = useState(false)
  const kindGlyph = item.targetKind === 'channel' ? '#' : item.targetKind === 'direct' ? '@' : '↳'
  const replyCount = item.replyCount

  const open = (): void => {
    void props.openDock(item.conversationId).catch((cause: unknown) => {
      props.onError(cause instanceof Error ? cause.message : String(cause))
    })
  }

  const markDone = (): void => {
    if (donePending) return
    setDonePending(true)
    props.markInboxDone(item.conversationId, item.lastActivitySeq)
      .catch((cause: unknown) => {
        props.onError(cause instanceof Error ? cause.message : String(cause))
      })
      .finally(() => { setDonePending(false) })
  }

  return (
    <div
      className={css.card}
      role="button"
      tabIndex={0}
      aria-label={`打开 ${item.targetName}`}
      onClick={open}
      onKeyDown={event => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          open()
        }
      }}
    >
      <div className={css.cardTop}>
        <span className={css.cardTarget}>
          <span className={css.cardKind}>{kindGlyph}</span>
          {item.targetName}
        </span>
        <button
          type="button"
          className={css.doneButton}
          aria-label="标记完成"
          title="标记完成；有新消息时会重新出现"
          disabled={donePending}
          onClick={event => {
            event.stopPropagation()
            markDone()
          }}
        >
          ✓
        </button>
      </div>
      {item.title !== '' ? <div className={css.cardTitle}>{item.title}</div> : null}
      <div className={css.cardMeta}>
        {item.latestReply !== undefined
          ? <span className={css.cardSender}>@{item.latestReply.senderName}</span>
          : null}
        <span>{relTime(item.lastActivityAtMs)}</span>
      </div>
      {item.latestReply !== undefined && item.latestReply.excerpt !== ''
        ? <p className={css.cardExcerpt}>{item.latestReply.excerpt}</p>
        : null}
      {item.task !== undefined || (replyCount !== undefined && replyCount !== '0')
        ? (
          <div className={css.cardChips}>
            {item.task !== undefined
              ? (
                <span className={css.chip} data-status={item.task.status}>
                  工作项 #{item.task.number} · {TASK_STATUS_TEXT[item.task.status] ?? item.task.status}
                </span>
              )
              : null}
            {replyCount !== undefined && replyCount !== '0'
              ? <span className={css.chip}>{replyCount} 条回复</span>
              : null}
          </div>
        )
        : null}
    </div>
  )
}

function ActivityPane(props: RegionProps & { state: ChaosClientState }): React.JSX.Element {
  const { state } = props
  const inbox = state.inbox
  const [error, setError] = useState<string | undefined>(undefined)
  const [morePending, setMorePending] = useState(false)

  useEffect(() => {
    if (inbox.status !== 'idle') return
    let cancelled = false
    props.ensure()
      .then(async () => { if (!cancelled) await props.loadInbox() })
      .catch(() => {})
    return () => { cancelled = true }
    // One-shot first load when the tab mounts.
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className={css.pane}>
      {error !== undefined
        ? <div className={css.paneError} role="alert">{error}</div>
        : null}
      <div className={css.list}>
        {inbox.status === 'loading' && inbox.items.length === 0
          ? <div className={css.empty}>加载中…</div>
          : null}
        {inbox.status === 'error' && inbox.items.length === 0
          ? (
            <div className={css.empty}>
              动态加载失败。
              <button
                type="button"
                className={css.retryButton}
                onClick={() => { void props.loadInbox() }}
              >
                重试
              </button>
            </div>
          )
          : null}
        {inbox.items.map(item => (
          <ActivityCard
            key={item.conversationId}
            {...props}
            item={item}
            onError={setError}
          />
        ))}
        {inbox.status === 'ready' && inbox.items.length === 0
          ? (
            <div className={css.empty}>
              协作动态都处理完了。
              <span className={css.emptyHint}>新的频道消息、Thread 回复和工作项更新会出现在这里。</span>
            </div>
          )
          : null}
        {inbox.nextCursor !== undefined
          ? (
            <button
              type="button"
              className={css.moreButton}
              disabled={morePending}
              onClick={() => {
                setMorePending(true)
                props.loadMoreInbox().finally(() => { setMorePending(false) })
              }}
            >
              {morePending ? '加载中…' : '加载更多'}
            </button>
          )
          : null}
      </div>
    </div>
  )
}
