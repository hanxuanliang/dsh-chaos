import type { ReactNode } from 'react'
import type { ChaosClientState } from './controller.ts'
import css from './ChaosPanel.module.css'

interface RoomCardRuntime {
  useChaos: (select: (state: ChaosClientState) => ChaosClientState) => ChaosClientState
  selectTarget: (targetId: string) => Promise<void>
  block?: {
    call?: { argsRaw?: string } | null
    meta?: unknown
  }
}

function textOf(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() !== '' ? value : undefined
}

function takeText(record: Record<string, unknown>, key: string): { [name: string]: string } {
  const value = textOf(record[key])
  return value === undefined ? {} : { [key]: value }
}

function cardFrom(block: RoomCardRuntime['block']): { targetId?: string; text?: string } {
  const meta = block?.meta
  if (meta !== null && typeof meta === 'object') {
    const record = meta as Record<string, unknown>
    return { ...takeText(record, 'targetId'), ...takeText(record, 'text') }
  }
  const raw = block?.call?.argsRaw
  if (raw === undefined) return {}
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>
    return { ...takeText(parsed, 'targetId'), ...takeText(parsed, 'text') }
  } catch {
    return {}
  }
}

/** Official-stream room card. Click opens the same workbench as `#`. */
export function RoomCard(props: RoomCardRuntime): ReactNode {
  const chaos = props.useChaos(value => value)
  const card = cardFrom(props.block)
  const target = chaos.targets.find(item => item.id === card.targetId)
  const title = target === undefined ? '房间' : `#${target.name}`
  const preview = card.text ?? '打开房间'

  return (
    <button
      type="button"
      className={css.roomCard}
      aria-label={`打开 ${title}`}
      onClick={() => {
        if (card.targetId !== undefined) void props.selectTarget(card.targetId)
      }}
    >
      <strong>{title}</strong>
      <span>{preview}</span>
    </button>
  )
}
