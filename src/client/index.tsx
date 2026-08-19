import { AgentSettingsCard } from './AgentSettingsCard.tsx'
import type { ChaosClientContext } from './dsh-compat.ts'
import { en, zh } from './locales.ts'

export const inject = ['slots', 'connection', 'locale']

/** Mount the P0 surface: the Agents management page inside DSH settings. */
export function apply(rawContext: unknown): void {
  const ctx = rawContext as ChaosClientContext
  ctx.effect(() => ctx.locale.register('chaos', { zh, en }), 'dsh-chaos: locale dictionaries')
  const translate = ctx.locale.bind('chaos')
  ctx.slots.inject('settings.section', () => ctx.slots.register({
    name: 'settings.section',
    id: 'chaos-agents',
    order: 40,
    label: () => translate('settings.tab'),
    locale: 'chaos',
    inject: () => ({ connection: ctx.connection, t: translate }),
  }, AgentSettingsCard))
}
