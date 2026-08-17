import { useEffect, useMemo, useState, type ReactNode } from 'react'
import type { InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type { ChaosClientState } from './controller.ts'
import type { NativeTarget } from '../native.ts'
import { applyHashPick, detectHashTrigger, rankChannels, type HashHit } from './hash-picker.ts'
import css from './ChaosPanel.module.css'

export interface HashPickerInjected {
  hooks: { chaos: { getSnapshot(): ChaosClientState; subscribe(listener: () => void): () => void } }
  selectTarget: (targetId: string) => Promise<void>
}

export type HashPickerProps = PropsRuntime<'conversation.input.overlay'> & InjectFace<HashPickerInjected>

interface OverlayInput {
  useInput?: (select: (state: { draft: string }) => string) => string
  inputActions?: { setDraft(text: string): void }
  useChaos: (select: (state: ChaosClientState) => ChaosClientState) => ChaosClientState
}

function liveCaret(): number | undefined {
  const active = document.activeElement
  if (!(active instanceof HTMLTextAreaElement)) return undefined
  return active.selectionStart
}

function channelRows(targets: readonly NativeTarget[]): readonly { id: string; name: string }[] {
  return targets.filter(target => target.kind === 'channel').map(target => ({
    id: target.id,
    name: target.name,
  }))
}

/** Official-box `#` menu. Pick enters the room workbench; it does not send. */
export function HashPicker(props: HashPickerProps): ReactNode {
  const overlay = props as HashPickerProps & OverlayInput
  const draft = overlay.useInput === undefined ? '' : overlay.useInput(state => state.draft)
  const chaos = overlay.useChaos(value => value)
  const [caret, setCaret] = useState(0)
  const [highlight, setHighlight] = useState(0)

  useEffect(() => {
    const sync = (): void => {
      const next = liveCaret()
      if (next !== undefined) setCaret(next)
    }
    sync()
    document.addEventListener('selectionchange', sync)
    document.addEventListener('keyup', sync, true)
    document.addEventListener('click', sync, true)
    return () => {
      document.removeEventListener('selectionchange', sync)
      document.removeEventListener('keyup', sync, true)
      document.removeEventListener('click', sync, true)
    }
  }, [draft])

  const hit = useMemo(() => detectHashTrigger(draft, caret), [draft, caret])
  const channels = useMemo(
    () => rankChannels(channelRows(chaos.targets), hit?.query ?? ''),
    [chaos.targets, hit],
  )

  useEffect(() => { setHighlight(0) }, [hit?.query, channels.length])

  const pick = (channelId: string, current: HashHit): void => {
    const next = applyHashPick(draft, current)
    overlay.inputActions?.setDraft(next.draft)
    void props.selectTarget(channelId)
  }

  useEffect(() => {
    if (hit === null) return
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.isComposing || event.keyCode === 229) return
      if (event.key === 'ArrowDown') {
        event.preventDefault()
        event.stopPropagation()
        setHighlight(index => Math.min(channels.length - 1, index + 1))
        return
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault()
        event.stopPropagation()
        setHighlight(index => Math.max(0, index - 1))
      }
    }
    document.addEventListener('keydown', onKeyDown, true)
    return () => { document.removeEventListener('keydown', onKeyDown, true) }
  }, [hit, channels.length])

  if (hit === null) return null

  return (
    <div className={css.hashMenu} role="listbox" aria-label="Channels">
      <div className={css.hashGroup}>CHANNELS</div>
      {channels.length === 0 && <div className={css.hashEmpty}>没有匹配的 Channel</div>}
      {channels.map((channel, index) => (
        <button
          key={channel.id}
          type="button"
          role="option"
          aria-selected={index === highlight}
          className={css.hashItem}
          data-active={index === highlight || undefined}
          onMouseDown={event => {
            event.preventDefault()
            pick(channel.id, hit)
          }}
        >
          <span className={css.targetGlyph} aria-hidden>#</span>
          <span>{channel.name}</span>
        </button>
      ))}
    </div>
  )
}
