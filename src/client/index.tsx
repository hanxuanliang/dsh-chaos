import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import {
  AgentsPage,
  ChaosDock,
  ChaosPanel,
  ChannelsPage,
  ThreadPage,
  type ChaosPanelInjected,
} from './ChaosPanel.tsx'
import { HashPicker } from './HashPicker.tsx'
import { ChaosClientController } from './controller.ts'
import { installChannelSendHook } from './send-hook.ts'
import { betterSidebarOf, type BetterSidebarLite } from './sidecar.ts'

declare module '@deepseek-ai/dsh-client-ui-slots' {
  interface SlotMap {
    'conversation.input.dock': {
      kind: 'list'
      scope: 'session-maybe'
    }
    'conversation.input.overlay': {
      kind: 'list'
      scope: 'session'
    }
  }
}

interface ClientSessions {
  list: {
    getSnapshot(): { current?: string }
    subscribe(listener: () => void): () => void
  }
  open(id: string): void
}

interface ClientWorkspaces {
  create(input: { path: string }): Promise<{ id: string; title?: string }>
  rename(id: string, title: string): Promise<unknown>
}

export const inject = ['slots', 'connection', 'sessions', 'workspaces'] as const

/** Browser plugin: official center stays; right workbench is sidecar or a same-shaped fallback. */
export function apply(ctx: ClientContext): void {
  const connection = ctx.get('connection') as unknown as ConnectionHandle
  const controller = new ChaosClientController(connection.rpc)
  const sessions = ctx.sessions as unknown as ClientSessions
  const workspaces = ctx.workspaces as unknown as ClientWorkspaces

  const syncHostSession = (): void => {
    controller.setHostSession(sessions.list.getSnapshot().current)
  }

  const openOfficialSession = (sessionId: string): void => {
    try {
      sessions.open(sessionId)
    } catch (error) {
      console.warn('[dsh-chaos] open session failed', error)
    }
    controller.setHostSession(sessionId)
    controller.openDesk()
  }

  const registerHome = async (workspacePath: string, title: string): Promise<void> => {
    try {
      const workspace = await workspaces.create({ path: workspacePath })
      if (workspace.title !== title) {
        await workspaces.rename(workspace.id, title)
      }
    } catch (error) {
      console.warn('[dsh-chaos] register workspace failed', error)
    }
  }

  const openAgent = (agentId: string): void => {
    const binding = controller.getSnapshot().bindings.find(item => item.agentId === agentId)
    controller.openDesk(agentId)
    if (binding !== undefined) openOfficialSession(binding.sessionId)
  }

  const createAgent = async (name: string): Promise<void> => {
    const created = await controller.createAgent(name)
    if (created.workspacePath !== '') {
      await registerHome(created.workspacePath, created.actor.displayName)
    }
    if (created.binding !== undefined) openOfficialSession(created.binding.sessionId)
    else controller.openDesk(created.actor.id)
  }

  const injectFace = (): ChaosPanelInjected => ({
    hooks: { chaos: controller },
    ensure: () => controller.ensure(),
    refresh: () => controller.refresh(),
    toggleRail: () => { controller.toggleRail() },
    openRail: () => { controller.openRail() },
    setRailTab: tab => { controller.setRailTab(tab) },
    openWorkbench: () => { controller.openWorkbench() },
    closeWorkbench: () => { controller.closeWorkbench() },
    setAsTask: asTask => { controller.setAsTask(asTask) },
    openDesk: agentId => { controller.openDesk(agentId) },
    closeSurface: () => { controller.closeSurface() },
    clearTarget: () => { controller.clearTarget() },
    selectTarget: targetId => controller.selectTarget(targetId),
    createChannel: name => controller.createChannel(name),
    createAgent,
    openAgent,
    createDirect: peerId => controller.createDirect(peerId),
    addMember: (targetId, memberId) => controller.addMember(targetId, memberId),
    createThread: rootMessageId => controller.createThread(rootMessageId),
    followThread: threadTargetId => controller.followThread(threadTargetId),
    unfollowThread: threadTargetId => controller.unfollowThread(threadTargetId),
    openThreadPanel: threadTargetId => controller.openThreadPanel(threadTargetId),
    closeThreadPanel: () => { controller.closeThreadPanel() },
    sendToThread: text => controller.sendToThread(text),
    send: async text => { await controller.send(text) },
    sendAsTask: text => controller.sendAsTask(text),
    createTask: messageId => controller.createTask(messageId),
  })

  const injectLater = (ctx as unknown as {
    inject?: (deps: readonly string[], factory: (scope: { conversation?: unknown }) => (() => void) | void) => void
  }).inject
  if (typeof injectLater === 'function') {
    injectLater(['conversation'], scope => installChannelSendHook(scope.conversation, controller))
  }

  ctx.effect(() => {
    const stopSessions = sessions.list.subscribe(syncHostSession)
    syncHostSession()
    const stopSend = typeof injectLater === 'function'
      ? () => {}
      : installChannelSendHook(
        (ctx as unknown as { conversation?: unknown }).conversation,
        controller,
      )
    return () => {
      stopSend()
      stopSessions()
      controller.dispose()
    }
  }, 'dsh-chaos: Client controller')

  ctx.slots.inject('conversation.input.dock', () => ctx.slots.register({
    name: 'conversation.input.dock',
    id: 'dsh-chaos-dock',
    order: 20,
    inject: injectFace,
  }, ChaosDock))

  ctx.slots.inject('conversation.input.overlay', () => ctx.slots.register({
    name: 'conversation.input.overlay',
    id: 'dsh-chaos-hash',
    order: 30,
    inject: injectFace,
  }, HashPicker))

  const sidecar = betterSidebarOf(ctx as unknown as { betterSidebar?: BetterSidebarLite })

  if (sidecar !== undefined) {
    ctx.effect(() => {
      const face = injectFace()
      const stopChannels = sidecar.registerTab({
        id: 'dsh-chaos:channels',
        title: 'Channels',
        order: 10,
        single: true,
        component: () => <ChannelsPage {...face} />,
      })
      const stopAgents = sidecar.registerTab({
        id: 'dsh-chaos:agents',
        title: 'Agents',
        order: 11,
        single: true,
        component: () => <AgentsPage {...face} />,
      })
      const stopThread = sidecar.registerTab({
        id: 'dsh-chaos:thread',
        title: 'Thread',
        order: 12,
        single: true,
        component: () => <ThreadPage {...face} />,
      })
      return () => {
        stopChannels()
        stopAgents()
        stopThread()
      }
    }, 'dsh-chaos: sidecar tabs')
    return
  }

  ctx.slots.inject('shell.overlay', () => {
    return ctx.slots.register({
      name: 'shell.overlay',
      id: 'dsh-chaos-workspace',
      order: 100,
      inject: injectFace,
    }, ChaosPanel)
  })
}
