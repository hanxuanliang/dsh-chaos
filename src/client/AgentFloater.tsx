import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { InjectFace } from '@deepseek-ai/dsh-client-ui-slots'
import type { ChaosInjected } from './Workbench.tsx'
import { useChaos } from './Workbench.tsx'
import css from './AgentFloater.module.css'

function BotGlyph(): React.JSX.Element {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="2.5" y="5" width="11" height="8" rx="2.5" stroke="currentColor" strokeWidth="1.4" />
      <path d="M8 5V2.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
      <circle cx="6" cy="9" r="1" fill="currentColor" />
      <circle cx="10" cy="9" r="1" fill="currentColor" />
    </svg>
  )
}

/**
 * agent-teams style floating roster: a body-portal badge listing the Agent
 * members of this DSH; click a row to open its bound official Session.
 * Mounted as a standalone `shell.overlay` entry so it stays visible
 * independent of the workbench open state.
 */
export function AgentFloater(props: InjectFace<ChaosInjected>): React.JSX.Element | null {
  const state = useChaos(props)
  const [open, setOpen] = useState(false)
  const badgeRef = useRef<HTMLButtonElement | null>(null)
  const panelRef = useRef<HTMLDivElement | null>(null)
  const agents = state.actors.filter(actor => actor.kind === 'agent')

  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        setOpen(false)
        badgeRef.current?.focus()
      }
    }
    const onPointerDown = (event: PointerEvent): void => {
      const panel = panelRef.current
      const badge = badgeRef.current
      if (!(event.target instanceof Node)) return
      if (panel?.contains(event.target) === true || badge?.contains(event.target) === true) return
      setOpen(false)
    }
    window.addEventListener('keydown', onKeyDown, true)
    document.addEventListener('pointerdown', onPointerDown)
    return () => {
      window.removeEventListener('keydown', onKeyDown, true)
      document.removeEventListener('pointerdown', onPointerDown)
    }
  }, [open])

  if (agents.length === 0) return null

  return createPortal(
    <>
      <button
        ref={badgeRef}
        type="button"
        className={css.badge}
        aria-label={`Agents（${String(agents.length)}）`}
        aria-expanded={open}
        onClick={() => { setOpen(current => !current) }}
      >
        <BotGlyph />
        Agents
        <span className={css.count}>{agents.length}</span>
      </button>
      {open
        ? (
          <div ref={panelRef} className={css.panel} role="dialog" aria-label="Agents 面板">
            <div className={css.head}>Agents</div>
            <div className={css.list}>
              {agents.map(agent => {
                const binding = state.bindings.find(item => item.agentId === agent.id)
                return (
                  <button
                    key={agent.id}
                    type="button"
                    className={css.row}
                    disabled={binding === undefined}
                    title={binding === undefined ? '未绑定 Session' : `打开 ${agent.displayName} 的 Session`}
                    onClick={() => {
                      props.openAgentSession(agent.id)
                      setOpen(false)
                    }}
                  >
                    <span className={css.avatar}>{agent.displayName.slice(0, 1)}</span>
                    <span className={css.meta}>
                      <span className={css.name}>{agent.displayName}</span>
                      <span className={css.sub}>
                        {binding !== undefined ? `${binding.provider} · ${binding.model}` : '未绑定 Session'}
                      </span>
                    </span>
                  </button>
                )
              })}
            </div>
          </div>
        )
        : null}
    </>,
    document.body,
  )
}
