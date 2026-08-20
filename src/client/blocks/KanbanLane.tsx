/**
 * KanbanLane — 泳道壳: 列头(状态点+标题+count) + 卡栈 + 空态 + dragover 接受态。
 * 消费 .card[data-plugin] 拱法, 色点 = 宿主 StateDot (in_progress 黄=warning 未失真)。
 * 平板板板自身定高 = 内容高(align-self: flex-start), 不撑满整栏。
 */
import type { ReactNode } from 'react'
import { StateDot, type StateDotState } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTask } from '../../native.ts'
import css from './KanbanLane.module.css'
import chipCss from '../atoms/StatusChip.module.css'

/** Lane 色点: 业务 ramp 无 review hue, in_review 用 chip 的本地紫点收编。 */
const LANE_DOT: Partial<Record<NativeTask['status'], StateDotState>> = {
  todo: 'ongoing',
  in_progress: 'warning',
  done: 'done',
}

export function KanbanLane({ status, label, count, dragOver, emptyLabel, onDragOver, onDragLeave, onDrop, children }: {
  status: NativeTask['status']
  label: string
  count: number
  dragOver: boolean
  emptyLabel: string
  onDragOver: (event: React.DragEvent<HTMLElement>) => void
  onDragLeave: () => void
  onDrop: (event: React.DragEvent<HTMLElement>) => void
  children: ReactNode
}) {
  const dot = LANE_DOT[status]
  return (
    <section
      className={css.column}
      data-status={status}
      data-drag-over={dragOver ? 'true' : undefined}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      <header className={css.columnHeader}>
        {dot !== undefined
          ? <StateDot state={dot} size={10} />
          : <span className={chipCss.dot} data-status={status} aria-hidden="true" />}
        <h3 className={css.columnTitle}>{label}</h3>
        <span className={css.columnCount}>{count}</span>
      </header>
      <div className={css.cards}>
        {children}
        {count === 0 && <div className={css.columnEmpty}>{emptyLabel}</div>}
      </div>
    </section>
  )
}

/** 泳道列表 grid (四列), 消者 kanban 板布 自己的 taskColumns。 */
export function KanbanLaneGrid({ children }: { children: ReactNode }) {
  return <div className={css.columns}>{children}</div>
}
