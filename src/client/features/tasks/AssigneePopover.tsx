/**
 * AssigneePopover — 任务认领/释放 popover (带 search 行 + agent 组)。
 * 从 ChannelTasksBoard 抽出的 raft 弹层形态; fixed 定位脱宿主 overflow(原设计),
 * root 带 [data-plugin] 同元素 0-2-0 特异性。Menu 原语仍不够这里的 search+值班
 * 行需求(其 footer 是平订行, 未含 input 原语) — popover 形态本体保留。
 */
import { useEffect, useRef, useState } from 'react'
import { IconCheckOutline16, IconEditOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import type { NativeActor, NativeTask } from '../../../native.ts'
import type { ChaosTranslate } from '../../locales.ts'
import css from './AssigneePopover.module.css'

function CheckGlyph(): JSX.Element {
  return <IconCheckOutline16 />
}
function PencilGlyph(): JSX.Element {
  return <IconEditOutline16 />
}

/** raft reference row: avatar + display name + trailing check on the current seat. */
function AssigneeRow({ actor, query, current, onPick }: {
  actor: NativeActor | undefined
  query: string
  current: boolean
  onPick: () => void
}): JSX.Element | undefined {
  if (actor === undefined) return undefined
  const q = query.trim().toLowerCase()
  if (q !== '' && !actor.handle.toLowerCase().includes(q) && !actor.displayName.toLowerCase().includes(q)) {
    return undefined
  }
  return (
    <button type="button" role="menuitemradio" aria-checked={current} className={css.menuItem} data-current={current ? 'true' : undefined} onClick={onPick}>
      <AvatarChip handle={actor.handle} displayName={actor.displayName} />
      <span className={css.rowLabel}>{actor.displayName || `@${actor.handle}`}</span>
      {current && <CheckGlyph />}
    </button>
  )
}

/** Raft assignee chip: bordered value + pencil, menu offers the few HONEST
 * actions the fixed-principal surface supports — claim (when the pool holds
 * the task) or unclaim self (when I hold it); someone else's claim is
 * read-only. */
export function AssigneePopover({ task, selfActor, agents, assigneeLabel, t, onClaim, onUnclaim }: {
  task: NativeTask
  /** 当前用户 actor（fixed-identity RPC 下的 UI 位）。 */
  selfActor: NativeActor | undefined
  /** channel members 中的 agent（后端 claim_task 自身会校验被认领方父级成员）。 */
  agents: NativeActor[]
  assigneeLabel: string | undefined
  t: ChaosTranslate
  onClaim: (actorId?: string) => void
  onUnclaim: () => void
}): JSX.Element {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const rootRef = useRef<HTMLSpanElement>(null)
  const anchorRef = useRef<HTMLButtonElement>(null)
  const [anchorRect, setAnchorRect] = useState<{ top: number; left: number } | undefined>(undefined)

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent): void => {
      if (rootRef.current?.contains(event.target as Node) !== true) setOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => { document.removeEventListener('mousedown', close) }
  }, [open])
  useEffect(() => { if (open) setQuery('') }, [open])

  const q = query.trim().toLowerCase()
  const selfHit = selfActor !== undefined && (q === '' || selfActor.handle.toLowerCase().includes(q) || selfActor.displayName.toLowerCase().includes(q))
  const agentHits = agents.filter(agent => q === '' || agent.handle.toLowerCase().includes(q) || agent.displayName.toLowerCase().includes(q))
  const filteredEmpty = !selfHit && agentHits.length === 0

  const mine = task.assigneeId !== undefined && selfActor !== undefined && task.assigneeId === selfActor.id
  const editable = task.assigneeId === undefined || mine
  if (!editable) {
    return <span className={css.chipStatic}>{assigneeLabel}</span>
  }
  return (
    <span ref={rootRef} className={css.dropdown}>
      <button
        ref={anchorRef}
        type="button"
        className={css.chip}
        data-plugin="dsh-chaos"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          if (!open) {
            // fixed 定位: host Modal overflow 会裁 absolute 下拉, 坐标脱离祖先链
            const rect = anchorRef.current?.getBoundingClientRect()
            if (rect !== undefined) setAnchorRect({ top: rect.bottom + 4, left: rect.left })
          }
          setOpen(v => !v)
        }}
      >
        {assigneeLabel ?? t('tasks.unassigned')}
        <PencilGlyph />
      </button>
      {open && (
        <span
          role="menu"
          className={css.menu}
          style={anchorRect === undefined ? undefined : { position: 'fixed', top: anchorRect.top, left: anchorRect.left }}
        >
          <span className={css.menuTitle}>{t('tasks.assigneeMenuTitle')}</span>
          <input
            className={css.menuSearch}
            type="search"
            placeholder={t('tasks.assigneeMenuSearch')}
            value={query}
            onChange={(event) => { setQuery(event.target.value) }}
            autoFocus
          />
          {/* raft 形态: Unassigned = 当前值指示行(选中态);仅「已指派且为本人」时可点 → 释放 */}
          <button
            type="button"
            role="menuitemradio"
            aria-checked={task.assigneeId === undefined}
            className={css.menuItem}
            data-current={task.assigneeId === undefined ? 'true' : undefined}
            disabled={task.assigneeId === undefined || !mine}
            onClick={() => { setOpen(false); onUnclaim() }}
          >
            <span className={css.rowLabel}>{t('tasks.unassigned')}</span>
            {task.assigneeId === undefined && <CheckGlyph />}
          </button>
          <AssigneeRow
            actor={selfActor}
            query={query}
            current={task.assigneeId !== undefined && task.assigneeId === selfActor?.id}
            onPick={() => { setOpen(false); onClaim() }}
          />
          {agentHits.map(agent => (
            <AssigneeRow
              key={agent.id}
              actor={agent}
              query={query}
              current={task.assigneeId === agent.id}
              onPick={() => { setOpen(false); onClaim(agent.id) }}
            />
          ))}
          {filteredEmpty && (
            <span className={css.menuEmpty}>{t('tasks.assigneeMenuEmpty')}</span>
          )}
        </span>
      )}
    </span>
  )
}
