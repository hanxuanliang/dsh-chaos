import { Fragment, type JSX, type ReactNode } from 'react'
import { IconChevronDownOutline14, IconSearchOutline16, StateDot } from '@deepseek-ai/dsh-client-ui-primitives'
import type { AgentProfile } from '../../agent-settings-types.ts'
import type { ChaosTranslate } from '../locales.ts'
import { AvatarChip } from '../atoms/AvatarChip.tsx'
import css from './AgentList.module.css'

export function AgentList({ profiles, total, query, selectedId, expandedContent, onQueryChange, onSelect, t }: {
  profiles: AgentProfile[]
  total: number
  query: string
  selectedId: string | undefined
  expandedContent?: ReactNode
  onQueryChange(value: string): void
  onSelect(profile: AgentProfile): void
  t: ChaosTranslate
}): JSX.Element {
  return <section className={css.list} aria-label={t('agents.title')}>
    <label className={css.search}>
      <IconSearchOutline16 size={16} />
      <span className={css.srOnly}>{t('agents.search')}</span>
      <input type="search" value={query} placeholder={t('agents.searchPlaceholder')}
        onChange={event => { onQueryChange(event.target.value) }} />
    </label>
    <div className={css.listHeading}><strong>{t('agents.list')}</strong><span>{profiles.length === total ? total : `${profiles.length} / ${total}`}</span></div>
    {profiles.length === 0
      ? <p className={css.noResults}>{t('agents.searchEmpty')}</p>
      : <nav className={css.grid} aria-label={t('agents.list')}>
        {profiles.map(profile => {
          const expanded = profile.actor.id === selectedId
          return <Fragment key={profile.actor.id}>
            <button type="button" className={css.card}
              data-agent-id={profile.actor.id}
              data-selected={expanded ? 'true' : undefined}
              aria-expanded={expanded}
              aria-controls={expanded ? `chaos-agent-${profile.actor.id}-detail` : undefined}
              onClick={() => { onSelect(profile) }}>
              <AvatarChip handle={profile.actor.handle} displayName={profile.actor.displayName} size="lg" />
              <span className={css.copy}>
                <strong>{profile.actor.displayName}</strong>
                <span>@{profile.actor.handle}</span>
                <small>{profile.charter.summary}</small>
              </span>
              <span className={css.status}>
                <StateDot state={profile.binding === undefined ? 'warning' : 'done'} size={8} />
                <span>{profile.binding === undefined ? t('agents.unconfigured') : t('agents.configured')}</span>
              </span>
              <IconChevronDownOutline14 size={14} className={expanded ? css.chevronOpen : css.chevron} />
            </button>
            {expanded && expandedContent !== undefined && <div className={css.expanded}>{expandedContent}</div>}
          </Fragment>
        })}
      </nav>}
  </section>
}
