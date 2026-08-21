import type { JSX } from 'react'
import { StateDot } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentProfile } from '../../agent-settings-types.ts'
import type { ChaosTranslate } from '../locales.ts'
import { AvatarChip } from '../atoms/AvatarChip.tsx'
import css from './AgentList.module.css'

export function AgentList({ profiles, selectedId, onSelect, t }: {
  profiles: AgentProfile[]
  selectedId: string | undefined
  onSelect(profile: AgentProfile): void
  t: ChaosTranslate
}): JSX.Element {
  return <nav className={css.list} aria-label={t('agents.title')}>
    {profiles.map(profile => <button key={profile.actor.id} type="button" className={css.row}
      data-agent-id={profile.actor.id}
      data-selected={profile.actor.id === selectedId ? 'true' : undefined}
      aria-current={profile.actor.id === selectedId ? 'true' : undefined}
      onClick={() => { onSelect(profile) }}>
      <AvatarChip handle={profile.actor.handle} displayName={profile.actor.displayName} size="md" />
      <span className={css.copy}>
        <strong>{profile.actor.displayName}</strong>
        <span>@{profile.actor.handle}</span>
        <small>{profile.charter.summary}</small>
      </span>
      <span className={css.state} title={profile.binding === undefined ? t('agents.unconfigured') : t('agents.configured')}>
        <StateDot state={profile.binding === undefined ? 'warning' : 'done'} size={8} />
      </span>
    </button>)}
  </nav>
}
