import { useEffect, useRef } from 'react'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import {
  Composer,
  MessageList,
  ThreadContextPanel,
  useChaos,
  type WorkbenchProps,
} from './Workbench.tsx'
import css from './Dock.module.css'

/**
 * Docked right-side conversation panel — the landing surface for Activity
 * inbox entries. Lives in the frame-wide shell.overlay layer beside the
 * workbench modal (the two never render together; the controller enforces
 * mutual exclusion). Thread replies take over the panel full-width instead of
 * a second column, mirroring the workbench's narrow-viewport drawer behavior.
 */
export function ConversationDock(props: WorkbenchProps): React.JSX.Element | null {
  const state = useChaos(props)
  const open = state.dock === 'open'
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const panelRef = useRef<HTMLDivElement | null>(null)
  const restoreFocusRef = useRef<Element | null>(null)

  useEffect(() => {
    if (!open) return
    restoreFocusRef.current = document.activeElement
    panelRef.current?.focus()
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') props.closeDock()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      const previous = restoreFocusRef.current
      if (previous instanceof HTMLElement) previous.focus()
    }
  }, [open]) // eslint-disable-line react-hooks/exhaustive-deps

  if (!open || selected === undefined) return null

  const kindGlyph = selected.kind === 'channel' ? '#' : selected.kind === 'direct' ? '@' : '↳'
  const inThread = state.threadPanelId !== undefined
  // Thread targets carry a machine name (thread:<id>); label them by their parent.
  const parentName = selected.kind === 'thread' && selected.parentTargetId !== undefined
    ? state.targets.find(target => target.id === selected.parentTargetId)?.name
    : undefined
  const title = parentName !== undefined ? `${parentName} 的 Thread` : selected.name

  return (
    <div
      ref={panelRef}
      className={css.dock}
      role="complementary"
      aria-label={`协作会话 ${title}`}
      tabIndex={-1}
    >
      <header className={css.dockHead}>
        <span className={css.dockKind}>{kindGlyph}</span>
        <span className={css.dockTitle}>{title}</span>
        <button
          type="button"
          className={css.dockClose}
          aria-label="关闭协作会话"
          onClick={() => { props.closeDock() }}
        >
          ✕
        </button>
      </header>
      {inThread
        ? (
          <div className={css.dockThread}>
            <ThreadContextPanel {...props} state={state} />
          </div>
        )
        : (
          <>
            <MessageList {...props} state={state} />
            <Composer
              {...props}
              state={state}
              draftKey={selected.id}
              onSend={props.send}
              placeholder="回复…"
              showMembers={false}
            />
          </>
        )}
    </div>
  )
}
