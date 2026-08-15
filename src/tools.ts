import type { Context } from '@deepseek-ai/cordis'
import { defineTool } from '@deepseek-ai/dsh-tools'
import type { Agent } from '@deepseek-ai/dsh-agent'
import type { CollabRuntimeApi } from './contracts.ts'
import type { RuntimeManager } from './runtime.ts'

const MESSAGE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    seq: { type: 'string', required: true },
    id: { type: 'string', required: true },
    targetId: { type: 'string', required: true },
    authorId: { type: 'string', required: true },
    clientRequestId: { type: 'string', required: true },
    text: { type: 'string', required: true },
    createdAtMs: { type: 'number', required: true },
  },
} as const

const requireAgent = (agent: Agent | undefined): Agent => {
  if (agent === undefined) throw new Error('message tools require a DSH Agent execution identity')
  return agent
}

/** Register collab tools inside one unpublished Agent scope. */
export function installCollabTools(
  agentCtx: Context,
  collab: CollabRuntimeApi,
  runtimes: RuntimeManager,
): void {
  agentCtx.on('agent/status', ({ agent, status }) => {
    if (status === 'idle') return runtimes.rearmUnconfirmed(agent)
  })

  agentCtx.tools.register(defineTool({
    name: 'message_check',
    description: 'Read pending collaboration messages addressed to this Agent. The wake notice contains no message bodies, so call this tool whenever the collab inbox says messages are pending.',
    parameters: {
      limit: { type: 'integer', description: 'Maximum messages to return, from 1 through 100. Defaults to 50.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          id: { type: 'string' },
          agentId: { type: 'string', required: true },
          sessionId: { type: 'string', required: true },
          generation: { type: 'string', required: true },
          messages: {
            type: 'array',
            required: true,
            items: {
              type: 'object',
              additionalProperties: false,
              properties: {
                deliveryId: { type: 'string', required: true },
                message: { ...MESSAGE_SCHEMA, required: true },
              },
            },
          },
          checkedAtMs: { type: 'number', required: true },
        },
      },
      render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
    },
    async execute(args, exec) {
      const limit = args.limit ?? 50
      if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
        throw new Error('limit must be an integer from 1 through 100')
      }
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      const batch = await collab.checkInbox(
        binding.agentId,
        binding.generation,
        binding.sessionId,
        limit,
      )
      if (batch.id !== undefined) runtimes.recordInboxBatch(binding, batch.id)
      return batch
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'message_send',
    description: 'Commit one text message to an exact collaboration target. Author identity is derived from the calling Agent and cannot be supplied by arguments.',
    parameters: {
      targetId: { type: 'string', required: true, description: 'Exact Channel, direct, or Thread target id from message_check/read.' },
      text: { type: 'string', required: true, description: 'Text to commit.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          message: { ...MESSAGE_SCHEMA, required: true },
          recipientIds: { type: 'array', required: true, items: { type: 'string' } },
          wakeAgentIds: { type: 'array', required: true, items: { type: 'string' } },
          replayed: { type: 'boolean', required: true },
        },
      },
      render: (_args, value) => [{
        type: 'text',
        text: `message ${value.message.id} committed to ${value.message.targetId}`,
      }],
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      return collab.sendMessage({
        targetId: args.targetId,
        authorId: binding.agentId,
        clientRequestId: String(exec.callId),
        text: args.text,
      })
    },
  }))
}
