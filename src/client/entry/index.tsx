import { AgentSettingsCard } from '../features/agents/AgentSettingsCard.tsx'
import { mountCollabWorkspace } from './mount.tsx'
import type { ChaosClientContext } from './dsh-compat.ts'
import { en, zh } from '../locales.ts'
import { navigateToCollabChannel } from './navigation.ts'

export const inject = ['slots', 'connection', 'locale', 'sessions', 'conversation', 'workspaces']

/** Mount the P0 surfaces: the Agents management page in DSH settings and the
 * sidebar-entry + center-overlay collab panel (P0-1 skeleton). */
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
    inject: () => ({
      connection: ctx.connection,
      openPath: (path: string) => ctx.workspaces.openPath(path),
      navigateChannel: navigateToCollabChannel,
      t: translate,
    }),
  }, AgentSettingsCard))
  ctx.effect(() => mountCollabWorkspace(ctx, translate), 'dsh-chaos: collab overlay')
}
