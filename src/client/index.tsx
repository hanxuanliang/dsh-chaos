import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import { ChaosClientController } from './controller.ts'
import { ChaosEntry, Workbench, type ChaosInjected } from './Workbench.tsx'
import { AgentFloater } from './AgentFloater.tsx'
import { AgentsSettings } from './AgentsSettings.tsx'
import type { NativeTask } from '../native.ts'

declare module '@deepseek-ai/dsh-client-ui-slots' {
  interface SlotMap {
    'settings.section': {
      kind: 'list'
      scope: 'root'
    }
  }
}

interface ClientSessions {
  open(id: string): void
}

interface ClientWorkspaces {
  create(input: { path: string }): Promise<{ id: string; title?: string }>
  rename(id: string, title: string): Promise<unknown>
}

export const inject = ['slots', 'connection', 'sessions', 'workspaces'] as const

/**
 * Collaboration workbench, rebuilt on official seams only:
 * a `sidebar.footer.action` entry beside Settings opens a `shell.overlay`
 * near-fullscreen workbench; an agent-teams style floater rosters Agents;
 * Agents are produced in a `settings.section` and consumed in channels.
 * No host private DOM, no sidecar services.
 */
export function apply(ctx: ClientContext): void {
  const connection = ctx.get('connection') as unknown as ConnectionHandle
  const controller = new ChaosClientController(connection.rpc)
  const sessions = ctx.sessions as unknown as ClientSessions | undefined
  const workspaces = ctx.workspaces as unknown as ClientWorkspaces | undefined

  const registerHome = async (workspacePath: string, title: string): Promise<void> => {
    if (workspaces === undefined) return
    try {
      const workspace = await workspaces.create({ path: workspacePath })
      if (workspace.title !== title) await workspaces.rename(workspace.id, title)
    } catch (error) {
      console.warn('[dsh-chaos] register workspace failed', error)
    }
  }

  const createAgent = async (name: string, presetId?: string) => {
    const presets = controller.getSnapshot().agentPresets
    const selected = presetId
      ?? presets.find(preset => preset.isDefault && preset.broken === undefined)?.id
      ?? presets.find(preset => preset.broken === undefined)?.id
    if (selected === undefined) throw new Error('当前 DSH 没有可用的 Agent Preset')
    const created = await controller.createAgent(name, selected)
    if (created.workspacePath !== '') {
      await registerHome(created.workspacePath, created.actor.displayName)
    }
    return created
  }

  const face = (): ChaosInjected => ({
    hooks: { chaos: controller },
    ensure: () => controller.ensure(),
    openWorkbench: () => {
      controller.openWorkbench()
      void controller.ensure()
    },
    closeWorkbench: () => { controller.closeWorkbench() },
    selectTarget: targetId => controller.selectTarget(targetId),
    createChannel: name => controller.createChannel(name),
    inviteAgent: async (targetId, channelName) => {
      const created = await createAgent(`${channelName} 助手`)
      await controller.addMember(targetId, created.actor.id)
    },
    createAgent: async (name, presetId) => {
      await createAgent(name, presetId)
    },
    openAgentSession: agentId => {
      const binding = controller.getSnapshot().bindings.find(item => item.agentId === agentId)
      if (binding === undefined || sessions === undefined) return
      try {
        sessions.open(binding.sessionId)
      } catch (error) {
        console.warn('[dsh-chaos] open agent session failed', error)
      }
    },
    send: text => controller.send(text).then(() => undefined),
    createThread: rootMessageId => controller.createThread(rootMessageId),
    openThreadPanel: threadTargetId => controller.openThreadPanel(threadTargetId),
    closeThreadPanel: () => { controller.closeThreadPanel() },
    sendToThread: text => controller.sendToThread(text),
    followThread: threadTargetId => controller.followThread(threadTargetId),
    unfollowThread: threadTargetId => controller.unfollowThread(threadTargetId),
    createTask: messageId => controller.createTask(messageId),
    claimTask: messageId => controller.claimTask(messageId),
    unclaimTask: (task: NativeTask) => controller.unclaimTask(task),
    updateTask: (task: NativeTask, status: NativeTask['status']) => controller.updateTask(task, status),
  })

  ctx.effect(() => () => { controller.dispose() }, 'dsh-chaos: controller')

  ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
    name: 'sidebar.footer.action',
    id: 'dsh-chaos-entry',
    order: 10,
    inject: face,
  }, ChaosEntry))

  ctx.slots.inject('shell.overlay', () => ctx.slots.register({
    name: 'shell.overlay',
    id: 'dsh-chaos-workbench',
    order: 100,
    inject: face,
  }, Workbench))

  ctx.slots.inject('shell.overlay', () => ctx.slots.register({
    name: 'shell.overlay',
    id: 'dsh-chaos-agent-floater',
    order: 90,
    inject: face,
  }, AgentFloater))

  ctx.slots.inject('settings.section', () => ctx.slots.register({
    name: 'settings.section',
    id: 'dsh-chaos-agents',
    order: 70,
    label: () => 'Agents',
    inject: face,
  }, AgentsSettings))
}
