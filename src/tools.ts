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

const TASK_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    messageId: { type: 'string', required: true },
    targetId: { type: 'string', required: true },
    number: { type: 'string', required: true },
    status: {
      type: 'string',
      required: true,
      enum: ['todo', 'in_progress', 'in_review', 'done'],
    },
    assigneeId: { type: 'string' },
    version: { type: 'string', required: true },
    createdAtMs: { type: 'number', required: true },
    updatedAtMs: { type: 'number', required: true },
  },
} as const

const requireAgent = (agent: Agent | undefined): Agent => {
  if (agent === undefined) throw new Error('message tools require a DSH Agent execution identity')
  return agent
}

const requireVersion = (value: string): string => {
  if (!/^[1-9][0-9]*$/.test(value)) {
    throw new Error('expectedVersion must be a positive decimal integer string')
  }
  return value
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
    name: 'message_read',
    description: 'Read one message or an ascending history page from one exact collaboration target. Thread access is inherited from its parent Channel or Direct target.',
    parameters: {
      targetId: { type: 'string', required: true, description: 'Exact Channel, Direct, or Thread target id.' },
      messageId: { type: 'string', description: 'Read exactly this message id within targetId.' },
      afterSeq: { type: 'string', description: 'For history reads, return messages after this decimal global sequence. Defaults to "0".' },
      limit: { type: 'integer', description: 'For history reads, return 1 through 100 messages. Defaults to 50.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          targetId: { type: 'string', required: true },
          messages: { type: 'array', required: true, items: MESSAGE_SCHEMA },
        },
      },
      render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
    },
    async execute(args, exec) {
      const limit = args.limit ?? 50
      if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
        throw new Error('limit must be an integer from 1 through 100')
      }
      const afterSeq = args.afterSeq ?? '0'
      if (!/^(0|[1-9][0-9]*)$/.test(afterSeq)) {
        throw new Error('afterSeq must be a non-negative decimal integer string')
      }
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      const messages = args.messageId === undefined
        ? await collab.readMessages(binding.agentId, args.targetId, afterSeq, limit)
        : [await collab.readMessage(binding.agentId, args.targetId, args.messageId)]
      return { targetId: args.targetId, messages }
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

  agentCtx.tools.register(defineTool({
    name: 'task_create',
    description: 'Attach Task metadata to one committed top-level Channel or Direct message. Actor identity comes from the calling Agent.',
    parameters: {
      messageId: { type: 'string', required: true, description: 'Top-level Message id to convert to a Task.' },
    },
    output: {
      schema: TASK_SCHEMA,
      render: (_args, value) => [{
        type: 'text',
        text: `task ${value.number} created for message ${value.messageId}`,
      }],
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      return collab.createTask(args.messageId, binding.agentId)
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'task_claim',
    description: 'Claim one existing Task by its source message id. A concurrent claim by another actor fails with a typed conflict.',
    parameters: {
      messageId: { type: 'string', required: true, description: 'Source Message id of the Task.' },
    },
    output: {
      schema: TASK_SCHEMA,
      render: (_args, value) => [{
        type: 'text',
        text: `task ${value.number} is ${value.status} and assigned to ${value.assigneeId ?? 'nobody'}`,
      }],
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      return collab.claimTask(args.messageId, binding.agentId)
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'task_list',
    description: 'List Tasks visible to the calling Agent, optionally within one exact collaboration target.',
    parameters: {
      targetId: { type: 'string', description: 'Optional exact Channel or Direct target id.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          tasks: { type: 'array', required: true, items: TASK_SCHEMA },
        },
      },
      render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      return { tasks: await collab.listTasks(binding.agentId, args.targetId) }
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'task_unclaim',
    description: 'Release a Task claimed by the calling Agent. Status is preserved and expectedVersion fences stale writes.',
    parameters: {
      messageId: { type: 'string', required: true, description: 'Source Message id of the Task.' },
      expectedVersion: { type: 'string', required: true, description: 'Current positive decimal Task version.' },
    },
    output: {
      schema: TASK_SCHEMA,
      render: (_args, value) => [{
        type: 'text',
        text: `task ${value.number} is unclaimed at version ${value.version}`,
      }],
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      return collab.unclaimTask(
        args.messageId,
        binding.agentId,
        requireVersion(args.expectedVersion),
      )
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'task_update',
    description: 'Move one visible Task to a valid lifecycle status. Assignment is unchanged and expectedVersion fences stale writes.',
    parameters: {
      messageId: { type: 'string', required: true, description: 'Source Message id of the Task.' },
      status: {
        type: 'string',
        required: true,
        enum: ['todo', 'in_progress', 'in_review', 'done'],
        description: 'Requested lifecycle status.',
      },
      expectedVersion: { type: 'string', required: true, description: 'Current positive decimal Task version.' },
    },
    output: {
      schema: TASK_SCHEMA,
      render: (_args, value) => [{
        type: 'text',
        text: `task ${value.number} moved to ${value.status} at version ${value.version}`,
      }],
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      return collab.updateTaskStatus(
        args.messageId,
        binding.agentId,
        args.status,
        requireVersion(args.expectedVersion),
      )
    },
  }))
}
