import type { JSX } from 'react'
import type { AgentProfile } from '../../../agent-settings-types.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import { PanelHeader, Toolbar } from '../../shared/layout/index.ts'
import { EmptyState, EntityRow, SearchField, StatusChip } from '../../shared/ui/index.ts'
import css from './AgentList.module.css'

export function AgentList({ profiles, total, query, selectedId, onQueryChange, onSelect, t }: {
  profiles: AgentProfile[]
  total: number
  query: string
  selectedId: string | undefined
  onQueryChange(value: string): void
  onSelect(profile: AgentProfile): void
  t: ChaosTranslate
}): JSX.Element {
  return (
    <section className={css.list} aria-label={t('agents.list')}>
      <PanelHeader title={t('agents.list')} description={profiles.length === total ? String(total) : `${profiles.length} / ${total}`} />
      <Toolbar start={(
        <SearchField
          className={css.search}
          value={query}
          onValueChange={onQueryChange}
          label={t('agents.search')}
          clearLabel={t('agents.clearSearch')}
          placeholder={t('agents.searchPlaceholder')}
        />
      )} />
      {profiles.length === 0
        ? <EmptyState title={t('agents.searchEmpty')} compact />
        : (
          <div className={css.items} role="list" aria-label={t('agents.list')}>
            {profiles.map(profile => {
              const selected = profile.actor.id === selectedId
              return (
                <div key={profile.actor.id} role="listitem">
                  <EntityRow
                    className={css.row}
                    leading={<AvatarChip handle={profile.actor.handle} displayName={profile.actor.displayName} size="lg" />}
                    title={profile.actor.displayName}
                    description={`@${profile.actor.handle} · ${profile.charter.summary}`}
                    selected={selected}
                    entityId={profile.actor.id}
                    ariaLabel={profile.actor.displayName}
                    onSelect={() => { onSelect(profile) }}
                    actions={(
                      <StatusChip
                        tone={profile.binding === undefined ? 'warning' : 'success'}
                        label={profile.binding === undefined ? t('agents.unconfigured') : t('agents.configured')}
                      />
                    )}
                  />
                </div>
              )
            })}
          </div>
        )}
    </section>
  )
}
