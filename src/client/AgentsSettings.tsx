import { useEffect, useRef, useState } from 'react'
import type { InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type { ChaosInjected } from './Workbench.tsx'
import { useChaos } from './Workbench.tsx'
import css from './AgentsSettings.module.css'

export type AgentsSettingsProps = PropsRuntime<'settings.section'> & InjectFace<ChaosInjected>

/**
 * Agents management surface inside the host Settings page (`settings.section`).
 * Production lives here; consumption (roster, invite) lives in the workbench.
 */
export function AgentsSettings(props: AgentsSettingsProps): React.JSX.Element {
  const state = useChaos(props)
  const [name, setName] = useState('')
  const [presetId, setPresetId] = useState<string | undefined>(undefined)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | undefined>(undefined)
  const nameRef = useRef<HTMLInputElement | null>(null)
  const agents = state.actors.filter(actor => actor.kind === 'agent')
  const presets = state.agentPresets.filter(preset => preset.broken === undefined)
  const defaultPreset = presets.find(preset => preset.isDefault) ?? presets[0]
  const selectedPreset = presets.find(preset => preset.id === presetId) ?? defaultPreset

  useEffect(() => { void props.ensure() }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const submit = async (): Promise<void> => {
    const trimmed = name.trim()
    if (trimmed === '' || pending || selectedPreset === undefined) return
    setPending(true)
    setError(undefined)
    try {
      await props.createAgent(trimmed, selectedPreset.id)
      setName('')
      nameRef.current?.focus()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setPending(false)
    }
  }

  return (
    <div className={css.page}>
      <section className={css.card} aria-label="新建 Agent">
        <h3 className={css.cardTitle}>新建 Agent</h3>
        <div className={css.form}>
          <input
            ref={nameRef}
            className={css.input}
            value={name}
            placeholder="名字，例如：前端助手"
            aria-label="Agent 名字"
            disabled={pending}
            onChange={event => {
              setName(event.target.value)
              if (error !== undefined) setError(undefined)
            }}
            onKeyDown={event => {
              if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
                event.preventDefault()
                void submit()
              }
            }}
          />
          <select
            className={css.select}
            aria-label="Agent Preset"
            value={selectedPreset?.id ?? ''}
            disabled={pending || presets.length === 0}
            onChange={event => { setPresetId(event.target.value) }}
          >
            {presets.map(preset => (
              <option key={preset.id} value={preset.id}>{preset.name ?? preset.id}</option>
            ))}
          </select>
          <button
            type="button"
            className={css.primaryButton}
            disabled={pending || name.trim() === '' || selectedPreset === undefined}
            onClick={() => { void submit() }}
          >
            {pending ? '创建中…' : '创建'}
          </button>
        </div>
        {presets.length === 0
          ? <p className={css.hint}>当前 DSH 没有可用的 Agent Preset。</p>
          : null}
        {error !== undefined ? <p className={css.error} role="alert">{error}</p> : null}
      </section>

      <section className={css.card} aria-label="Agents 列表">
        <h3 className={css.cardTitle}>Agents（{agents.length}）</h3>
        {agents.length === 0
          ? <p className={css.hint}>还没有 Agent。创建一个，然后把它邀请进频道一起干活。</p>
          : (
            <div className={css.list}>
              {agents.map(agent => {
                const binding = state.bindings.find(item => item.agentId === agent.id)
                const preset = state.agentPresets.find(item => item.id === binding?.preset)
                return (
                  <div key={agent.id} className={css.row}>
                    <span className={css.avatar}>{agent.displayName.slice(0, 1)}</span>
                    <span className={css.meta}>
                      <span className={css.name}>
                        {agent.displayName}
                        <span className={css.handle}> @{agent.handle}</span>
                      </span>
                      <span className={css.sub}>
                        {binding !== undefined
                          ? `${preset?.name ?? binding.preset} · ${binding.provider} · ${binding.model}`
                          : '未绑定 Runtime'}
                      </span>
                    </span>
                    <button
                      type="button"
                      className={css.ghostButton}
                      disabled={binding === undefined}
                      onClick={() => { props.openAgentSession(agent.id) }}
                    >
                      打开 Session
                    </button>
                  </div>
                )
              })}
            </div>
          )}
      </section>
    </div>
  )
}
