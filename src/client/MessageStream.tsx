/**
 * Channel message stream (spec §1.2), aligned to plocal-web's MessageScroller /
 * MessageBody anatomy on host tokens:
 * - Rows: full row = 28px avatar + 8px gap + body column (name row + body);
 *   compact rows (same author ≤5min) render a same-width empty placeholder so
 *   both body columns share one left edge, with a hover-revealed absolute
 *   gutter time. mb-1 / px-8 py-1 / hover tint; full rows add mt-1.5.
 * - Sticky centered day-divider pills (Today/Yesterday/month-day-weekday).
 * - Body renderer = react-markdown (remark-gfm + remark-breaks) with a
 *   host-token components map (plocal §4 shapes) and a micro rehype pass for
 *   @mention spans — v10 has no components.text hook (text nodes bypass the
 *   components map), so the split happens on hast text nodes, skipping
 *   code/pre/a subtrees by construction. Images render as links (no external
 *   image fetching in the panel).
 * - plocal TaskChip: status icon + #N (+ @assignee), 4-state icon/color only.
 * - Long bodies clamp at 344→320px with a bottom fade + Show more/Collapse.
 * - Head: "load older" button (forward paging; backend has no before-cursor)
 *   or a "beginning of messages" hint; tail keeps the conn-bar/resync faces.
 * P0 still skips virtual scrolling, thread previews, and hover reply.
 */
import { isValidElement, useEffect, useLayoutEffect, useMemo, useRef, useState, type JSX, type ReactNode, type UIEvent } from 'react'
import Markdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkBreaks from 'remark-breaks'
import type { NativeActor, NativeMessage, NativeTask } from '../native.ts'
import { avatarSeed } from './avatar.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './CollabPanel.module.css'

export interface MessageStreamProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channelId: string
  activeLocale(): string
  onOpenTasks(): void
}

const COMPACT_WINDOW_MS = 5 * 60 * 1000
const SKELETON_ROWS = [0, 1, 2]
/** plocal MessageBody clamp: fold only when the rendered body passes 344px; the
 * 320px collapsed cap lives in CollabPanel.module.css (.msgText[data-clamped]). */
const CLAMP_TRIGGER_PX = 344
const REMARK_PLUGINS = [remarkGfm, remarkBreaks]

function pad2(value: number): string {
  return String(value).padStart(2, '0')
}

function timeLabel(ms: number): string {
  const date = new Date(ms)
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`
}

function sameDay(left: Date, right: Date): boolean {
  return left.getFullYear() === right.getFullYear()
    && left.getMonth() === right.getMonth()
    && left.getDate() === right.getDate()
}

function dividerLabel(ms: number, t: ChaosTranslate, activeLocale: () => string): string {
  const date = new Date(ms)
  const now = new Date()
  if (sameDay(date, now)) return t('stream.today')
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)
  if (sameDay(date, yesterday)) return t('stream.yesterday')
  return activeLocale() === 'zh'
    ? date.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric', weekday: 'long' })
    : date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', weekday: 'long' })
}

function fullTimeTitle(ms: number, activeLocale: () => string): string {
  return new Date(ms).toLocaleString(activeLocale() === 'zh' ? 'zh-CN' : 'en-US')
}

/** plocal mentionIdsFromBody's unicode-boundary form; a token highlights only when it names a known actor. */
const MENTION = /(^|[^\p{L}\p{N}_@])@([^\s@]+)(?=$|[^\p{L}\p{N}_])/gu

/** Structural hast subset — we only walk element/text nodes, so no dep on hast types. */
interface HastLikeNode {
  type: string
  value?: string
  tagName?: string
  properties?: Record<string, unknown>
  children?: HastLikeNode[]
}

/**
 * The leading guard char stays plain text; only the `@token` segment becomes a
 * highlighted span (a leading space/CJK char must not inherit the accent).
 * Returns null when nothing matches so untouched subtrees keep their identity.
 */
function splitMentionText(value: string, names: ReadonlySet<string>): HastLikeNode[] | null {
  const out: HastLikeNode[] = []
  let last = 0
  let hit = false
  for (const match of value.matchAll(MENTION)) {
    const at = match.index
    const lead = match[1] ?? ''
    const token = match[2] ?? ''
    if (!names.has(token.toLowerCase())) continue
    if (at + lead.length > last) out.push({ type: 'text', value: value.slice(last, at + lead.length) })
    out.push({
      type: 'element',
      tagName: 'span',
      properties: { className: [css.mention] },
      children: [{ type: 'text', value: `@${token}` }],
    })
    last = at + match[0].length
    hit = true
  }
  if (!hit) return null
  if (last < value.length) out.push({ type: 'text', value: value.slice(last) })
  return out
}

/**
 * @mention highlighting at the hast text-node level (react-markdown v10 has no
 * components.text hook): split text nodes carrying a known `@name`, skip
 * a/code/pre subtrees — code spans and fenced blocks can never highlight.
 * Zero new dependencies: a rehype plugin is just a tree transform function.
 */
function makeMentionRehype(names: ReadonlySet<string>): () => (tree: HastLikeNode) => void {
  const SKIP = new Set(['a', 'code', 'pre'])
  const walk = (node: HastLikeNode): void => {
    if (node.type === 'element' && node.tagName !== undefined && SKIP.has(node.tagName)) return
    if (node.children === undefined) return
    const next: HastLikeNode[] = []
    let changed = false
    for (const child of node.children) {
      const split = child.type === 'text' && child.value !== undefined ? splitMentionText(child.value, names) : null
      if (split !== null) {
        next.push(...split)
        changed = true
        continue
      }
      next.push(child)
      walk(child)
    }
    if (changed) node.children = next
  }
  return () => (tree) => { walk(tree) }
}

/** Plain-text content of a rendered subtree (for the code-block Copy button). */
function textOf(node: ReactNode): string {
  if (node === null || node === undefined || typeof node === 'boolean') return ''
  if (typeof node === 'string') return node
  if (typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(textOf).join('')
  if (isValidElement(node)) return textOf((node.props as { children?: ReactNode }).children)
  return ''
}

/** Fenced block shell: dark slab + hover-revealed 28px copy button (→ ✓ for 1.2s). */
function MarkdownPre({ t, children }: { t: ChaosTranslate; children?: ReactNode }): JSX.Element {
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<number | undefined>(undefined)
  useEffect(() => () => { window.clearTimeout(timerRef.current) }, [])
  const label = t(copied ? 'stream.copied' : 'stream.copy')
  return (
    <div className={css.preShell}>
      <pre className={css.mdPre}>{children}</pre>
      <button
        type="button"
        className={css.codeCopy}
        aria-label={label}
        title={label}
        onClick={() => {
          navigator.clipboard.writeText(textOf(children).replace(/\n$/, '')).then(() => {
            setCopied(true)
            window.clearTimeout(timerRef.current)
            timerRef.current = window.setTimeout(() => { setCopied(false) }, 1200)
          }, () => {
            // Clipboard unavailable (permissions / insecure context) — inert.
          })
        }}
      >
        {copied
          ? <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3.5 8.5 6.5 11.5 12.5 4.5" /></svg>
          : <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5" /><path d="M10.5 5.5V3.5A1.5 1.5 0 0 0 9 2H4a1.5 1.5 0 0 0-1.5 1.5V9A1.5 1.5 0 0 0 4 10.5h1.5" /></svg>}
      </button>
    </div>
  )
}

/** plocal MessageBody's components map (truth §4), recolored to host tokens in CSS. */
function buildComponents(t: ChaosTranslate): Components {
  return {
    p: (props) => <p className={css.mdP}>{props.children}</p>,
    a: (props) => (
      <a className={css.mdLink} href={props.href} target="_blank" rel="noreferrer noopener">{props.children}</a>
    ),
    // Inline pill + fenced body share this element; .preShell resets the pill.
    code: (props) => (
      <code className={props.className === undefined ? css.mdCode : `${css.mdCode} ${props.className}`}>{props.children}</code>
    ),
    pre: (props) => <MarkdownPre t={t}>{props.children}</MarkdownPre>,
    // No external image fetching in the panel: render images as plain links.
    img: (props) => {
      const href = props.src ?? ''
      const alt = props.alt ?? ''
      return (
        <a className={css.mdLink} href={href} target="_blank" rel="noreferrer noopener">{alt !== '' ? alt : href}</a>
      )
    },
    h1: (props) => <h1 className={css.mdH1}>{props.children}</h1>,
    h2: (props) => <h2 className={css.mdH2}>{props.children}</h2>,
    h3: (props) => <h3 className={css.mdH3}>{props.children}</h3>,
    h4: (props) => <h4 className={css.mdH4}>{props.children}</h4>,
    ul: (props) => <ul className={css.mdUl}>{props.children}</ul>,
    ol: (props) => <ol className={css.mdOl}>{props.children}</ol>,
    li: (props) => <li className={css.mdLi}>{props.children}</li>,
    blockquote: (props) => <blockquote className={css.mdBlockquote}>{props.children}</blockquote>,
    hr: () => <hr className={css.mdHr} />,
    table: (props) => <div className={css.tableWrap}><table className={css.mdTable}>{props.children}</table></div>,
    th: (props) => <th className={css.mdTh} style={props.style}>{props.children}</th>,
    td: (props) => <td className={css.mdTd} style={props.style}>{props.children}</td>,
  }
}

/** Markdown body + plocal clamp (344 trigger / 320 cap / bottom fade / Show more). */
function MessageBody({ t, text, names }: { t: ChaosTranslate; text: string; names: ReadonlySet<string> }): JSX.Element {
  const bodyRef = useRef<HTMLDivElement | null>(null)
  const [oversize, setOversize] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const mentionRehype = useMemo(() => makeMentionRehype(names), [names])
  const components = useMemo(() => buildComponents(t), [t])

  useLayoutEffect(() => {
    const el = bodyRef.current
    if (el !== null) setOversize(el.scrollHeight > CLAMP_TRIGGER_PX)
  }, [text])

  const clamped = oversize && !expanded
  return (
    <>
      <div ref={bodyRef} className={css.msgText} data-clamped={clamped || undefined}>
        <Markdown remarkPlugins={REMARK_PLUGINS} rehypePlugins={[mentionRehype]} components={components}>
          {text}
        </Markdown>
        {clamped && <div className={css.clampFade} aria-hidden="true" />}
      </div>
      {oversize && (
        <button type="button" className={css.clampToggle} onClick={() => { setExpanded(value => !value) }}>
          {t(expanded ? 'stream.collapse' : 'stream.showMore')}
        </button>
      )}
    </>
  )
}

function StatusIcon({ status }: { status: NativeTask['status'] }): JSX.Element {
  // 10×10 stroke glyphs, hand-drawn per plocal's set (Circle / Play / Eye /
  // CircleCheck); no icon library on purpose.
  let shape: ReactNode
  switch (status) {
    case 'todo':
      shape = <circle cx="5" cy="5" r="3.2" />
      break
    case 'in_progress':
      shape = <path d="M3.2 1.8 8 5 3.2 8.2Z" />
      break
    case 'in_review':
      shape = (
        <>
          <path d="M1 5c1.2-1.8 2.5-2.7 4-2.7s2.8.9 4 2.7c-1.2 1.8-2.5 2.7-4 2.7s-2.8-.9-4-2.7Z" />
          <circle cx="5" cy="5" r="1" />
        </>
      )
      break
    case 'done':
      shape = (
        <>
          <circle cx="5" cy="5" r="3.2" />
          <path d="m3.4 5.1 1.1 1.1 2.1-2.3" />
        </>
      )
      break
  }
  return (
    <svg viewBox="0 0 10 10" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {shape}
    </svg>
  )
}

/** plocal chip: icon + #N (+ @assignee) — no "task" prefix, no status word. */
function TaskChip({ task, assignee, onOpenTasks }: {
  task: NativeTask
  assignee: string | undefined
  onOpenTasks(): void
}): JSX.Element {
  return (
    <button type="button" className={css.taskChip} data-status={task.status} onClick={onOpenTasks}>
      <StatusIcon status={task.status} />
      <span className={css.taskChipId}>#{task.number}</span>
      {assignee !== undefined && <span className={css.taskChipAssignee}>@{assignee}</span>}
    </button>
  )
}

interface StreamItem {
  kind: 'divider' | 'message'
  key: string
  label?: string
  message?: NativeMessage
  compact?: boolean
}

function buildItems(messages: NativeMessage[], t: ChaosTranslate, activeLocale: () => string): StreamItem[] {
  const items: StreamItem[] = []
  let previousDay = ''
  let previous: NativeMessage | undefined
  for (const message of messages) {
    const date = new Date(message.createdAtMs)
    const day = `${String(date.getFullYear())}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
    if (day !== previousDay) {
      items.push({ kind: 'divider', key: `day-${day}`, label: dividerLabel(message.createdAtMs, t, activeLocale) })
      previousDay = day
      previous = undefined
    }
    const compact = previous !== undefined
      && previous.authorId === message.authorId
      && message.createdAtMs - previous.createdAtMs <= COMPACT_WINDOW_MS
    items.push({ kind: 'message', key: message.id, message, compact })
    previous = message
  }
  return items
}

export function MessageStream({ t, store, state, channelId, activeLocale, onOpenTasks }: MessageStreamProps): JSX.Element {
  const messages = state.messagesByChannel[channelId]
  const total = state.totalByChannel[channelId]
  const actorsById = useMemo(() => {
    const mapped = new Map<string, NativeActor>()
    for (const actor of state.actors) mapped.set(actor.id, actor)
    return mapped
  }, [state.actors])
  const mentionNames = useMemo(() => {
    const names = new Set<string>()
    for (const actor of state.actors) {
      names.add(actor.handle.toLowerCase())
      names.add(actor.displayName.toLowerCase())
    }
    return names
  }, [state.actors])
  const items = useMemo(
    () => buildItems(messages ?? [], t, activeLocale),
    [messages, t, activeLocale],
  )

  // Auto-scroll: stick to the bottom while the user is near it; keep the
  // viewport anchored by height delta across a "load older" prepend.
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const pinnedRef = useRef(true)
  const channelRef = useRef(channelId)
  const olderAnchor = useRef<{ loading: boolean; height: number }>({ loading: false, height: 0 })

  useLayoutEffect(() => {
    const el = scrollerRef.current
    if (el === null) return
    if (channelRef.current !== channelId) {
      channelRef.current = channelId
      pinnedRef.current = true
    }
    const anchor = olderAnchor.current
    if (state.olderLoading) {
      anchor.loading = true
      anchor.height = el.scrollHeight
    } else if (anchor.loading) {
      anchor.loading = false
      el.scrollTop += el.scrollHeight - anchor.height
    }
    if (pinnedRef.current) el.scrollTop = el.scrollHeight
  }, [channelId, messages, state.olderLoading])

  const onScroll = (event: UIEvent<HTMLDivElement>): void => {
    const el = event.currentTarget
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60
  }

  if (state.connection === 'resyncing') {
    return (
      <div className={css.stream} role="status">
        <div className={css.chatCol}>
          <p className={css.resyncNote}>{t('channel.resyncing')}</p>
          {SKELETON_ROWS.map(row => <div key={row} className={css.skeletonRow} />)}
        </div>
      </div>
    )
  }

  const hasMore = messages !== undefined && total !== undefined && messages.length < total
  const showBeginning = !hasMore
    && messages !== undefined
    && messages.length > 0
    && !state.historyLoading
    && state.historyError === undefined

  return (
    <div className={css.streamWrap}>
      {state.connection === 'down' && (
        <div className={css.connBar} role="status">{t('channel.disconnected')}</div>
      )}
      <div className={css.stream} ref={scrollerRef} onScroll={onScroll}>
        <div className={css.chatCol}>
          {state.historyLoading && (
            <div className={css.skeletonStack} role="status" aria-label={t('channel.loading')}>
              {SKELETON_ROWS.map(row => <div key={row} className={css.skeletonRow} />)}
            </div>
          )}
          {messages !== undefined && messages.length === 0 && !state.historyLoading && state.historyError === undefined && (
            <div className={css.streamEmpty}>
              <svg viewBox="0 0 16 16" width="24" height="24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M2.5 3.5h11v8h-7l-4 3v-11Z" />
              </svg>
              <p>{t('channel.empty')}</p>
            </div>
          )}
          {hasMore && (
            <div className={css.olderRow}>
              {state.olderLoading
                ? SKELETON_ROWS.map(row => <div key={row} className={css.skeletonRow} />)
                : (
                  <button type="button" className={css.olderButton} onClick={() => { void store.loadOlder() }}>
                    {t('channel.loadOlder')}
                  </button>
                )}
            </div>
          )}
          {showBeginning && <p className={css.headHint}>{t('stream.beginning')}</p>}
          {items.map((item) => {
            if (item.kind === 'divider') {
              return <div key={item.key} className={css.dayDivider}><span>{item.label}</span></div>
            }
            const message = item.message as NativeMessage
            const author = actorsById.get(message.authorId)
            const task: NativeTask | undefined = state.tasksByMessage[message.id]
            const showTask = task !== undefined && task.targetId === channelId
            const assignee = showTask && task.assigneeId !== undefined
              ? actorsById.get(task.assigneeId)?.handle
              : undefined
            if (item.compact === true) {
              return (
                <div key={item.key} className={css.msgCompact}>
                  <div className={css.msgAvatarPlaceholder} aria-hidden="true" />
                  <div className={css.msgBody}>
                    <MessageBody t={t} text={message.text} names={mentionNames} />
                    {showTask && <TaskChip task={task} assignee={assignee} onOpenTasks={onOpenTasks} />}
                  </div>
                  <span className={css.msgGutterTime} title={fullTimeTitle(message.createdAtMs, activeLocale)}>
                    {timeLabel(message.createdAtMs)}
                  </span>
                </div>
              )
            }
            const handle = author?.handle ?? message.authorId
            const displayName = author?.displayName ?? message.authorId
            const seed = avatarSeed(handle, displayName)
            const binding = author === undefined ? undefined : state.bindingsByAgent[author.id]
            return (
              <div key={item.key} className={css.msg}>
                <span className={css.avatar} style={{ background: seed.background }} aria-hidden="true">{seed.initial}</span>
                <div className={css.msgMain}>
                  <div className={css.msgHead}>
                    <span className={css.msgName}>{displayName}</span>
                    <span className={css.msgTime}>{timeLabel(message.createdAtMs)}</span>
                    {author?.kind === 'agent' && (
                      <span className={css.msgBadge}>
                        AGENT{binding !== undefined ? ` · ${binding.model}` : ''}
                      </span>
                    )}
                  </div>
                  <div className={css.msgBody}>
                    <MessageBody t={t} text={message.text} names={mentionNames} />
                    {showTask && <TaskChip task={task} assignee={assignee} onOpenTasks={onOpenTasks} />}
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
