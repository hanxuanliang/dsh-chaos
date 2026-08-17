import { useEffect, useState, type FormEvent, type ReactNode } from 'react'
import type { InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type { ChaosPanelInjected } from './ChaosPanel.tsx'
import css from './ChaosPanel.module.css'

export type AgentsSettingsProps = PropsRuntime<'settings.section'> & InjectFace<ChaosPanelInjected> & {
  close?: () => void
}

/** Settings page: name-only create plus the Agent list. A row closes settings and opens that desk. */
export function AgentsSettings(props: AgentsSettingsProps): ReactNode {
  const state = props.useChaos(value => value)
  const [name, setName] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const agents = state.actors.filter(actor => actor.kind === 'agent')

  useEffect(() => { void props.ensure() }, [props.ensure])

  const create = (event: FormEvent): void => {
    event.preventDefault()
    const next = name.trim()
    if (next === '' || pending) return
    setPending(true)
    setError(null)
    void props.createAgent(next).then(
      () => {
        setName('')
        setPending(false)
      },
      failure => {
        setPending(false)
        setError(failure instanceof Error ? failure.message : String(failure))
      },
    )
  }

  return (
    <section className={css.settingsPage} aria-label="Agents">
      <p className={css.settingsLead}>只填一个名字。家和 Session 自己建。</p>
      <form className={css.settingsCreate} onSubmit={create}>
        <input
          value={name}
          onChange={event => { setName(event.target.value) }}
          placeholder="Agent 名称"
          aria-label="Agent 名称"
        />
        <button type="submit" className={css.primaryButton} disabled={pending || name.trim() === ''}>
          {pending ? '创建中…' : '创建'}
        </button>
      </form>
      {error !== null && <div className={css.dialogError} role="alert">{error}</div>}
      <nav className={css.settingsList} aria-label="Agents">
        {agents.length === 0 && <p className={css.empty}>还没有 Agent。</p>}
        {agents.map(actor => (
          <button
            key={actor.id}
            type="button"
            className={css.settingsRow}
            onClick={() => {
              props.openAgent(actor.id)
              props.close?.()
            }}
          >
            <strong>{actor.displayName}</strong>
            <span>跟随本机</span>
          </button>
        ))}
      </nav>
    </section>
  )
}
