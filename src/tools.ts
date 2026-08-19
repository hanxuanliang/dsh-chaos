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
    /** Authoritative snippet the store joins on every Task read — declare it or
     * additionalProperties:false rejects every tool envelope. */
    anchorText: { type: 'string' },
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
const UUID_TARGET_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
const SHORT_ID_RE = /^[0-9a-f]{4,16}$/i

/**
 * B 档 target 统一寻址（tae transport 形态翻译）：tools 接受
 *   UUID 直值                     → 原样使用
 *   '#channel'                    → 频道 target
 *   'dm:@handle'                  → direct target（按目标名含 handle 匹配）
 *   '#channel:<root 消息短 id>'    → EnsureThread 幂等建（或返回既有）后取 thread target
 *   'dm:@handle:<root 消息短 id>'  → 同上
 * tae 真值（server/application/target.go resolveThreadTarget）：复合引用
 * 解析到 root 后内部 EnsureThread——不要求 agent 先拿 thread UUID。
 */
async function resolveAgentTarget(
  collab: CollabRuntimeApi,
  agentId: string,
  ref: { targetId?: string | undefined; target?: string | undefined },
): Promise<{ targetId: string; display: string }> {
  const { targetId, target } = ref
  if (targetId !== undefined && target !== undefined) {
    throw new Error('targetId and target are mutually exclusive; pass exactly one')
  }
  if (targetId !== undefined) return { targetId, display: targetId }
  if (target === undefined || target === '') throw new Error('target or targetId is required')
  if (UUID_TARGET_RE.test(target)) return { targetId: target, display: target }

  const parsed = parseTargetText(target)
  const snapshot = await collab.snapshot(agentId)
  const base = parsed.kind === 'channel'
    ? snapshot.targets.find((target) => target.kind === 'channel' && target.name === parsed.name)
    : snapshot.targets.find((target) => target.kind === 'direct' && target.name.includes(parsed.name))
  if (base === undefined) {
    throw new Error(`unknown ${parsed.kind} target '${parsed.name}'`)
  }
  const rootShort = parsed.rootShort
  if (rootShort === undefined) {
    return { targetId: base.id, display: parsed.raw }
  }
  if (!SHORT_ID_RE.test(rootShort)) {
    throw new Error(`invalid root message short id '${rootShort}' (expected 4-16 hex chars)`)
  }
  // tae resolveThreadRoot: 短 id 前缀在调用者可见范围内匹配。
  const page = await collab.readMessages(agentId, base.id, '0', 100)
  const root = page.find(msg => msg.id.startsWith(rootShort) || msg.id.replaceAll('-', '').startsWith(rootShort))
  if (root === undefined) {
    throw new Error(`no message with short id '${rootShort}' in the first 100 messages of '${parsed.name}'`)
  }
  const thread = await collab.createThread(root.id, agentId)
  return { targetId: thread.id, display: parsed.raw }
}

function parseTargetText(raw: string): { kind: 'channel' | 'direct'; name: string; rootShort: string | undefined; raw: string } {
  const trimmed = raw.trim()
  if (trimmed.startsWith('#')) {
    const body = trimmed.slice(1)
    const sep = body.indexOf(':')
    const name = sep === -1 ? body : body.slice(0, sep)
    const rootShort = sep === -1 ? undefined : body.slice(sep + 1)
    if (name === '') throw new Error(`invalid channel target '${raw}'`)
    return { kind: 'channel', name, rootShort: rootShort === '' ? undefined : rootShort, raw: trimmed }
  }
  if (trimmed.startsWith('dm:@')) {
    const body = trimmed.slice(4)
    const sep = body.indexOf(':')
    const name = sep === -1 ? body : body.slice(0, sep)
    const rootShort = sep === -1 ? undefined : body.slice(sep + 1)
    if (name === '') throw new Error(`invalid direct target '${raw}'`)
    return { kind: 'direct', name, rootShort: rootShort === '' ? undefined : rootShort, raw: trimmed }
  }
  throw new Error(`invalid target '${raw}': expected UUID, '#channel', 'dm:@handle', or '<base>:<message-short-id>'`)
}

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
      targetId: { type: 'string', description: 'Exact UUID target id. Mutually exclusive with target.' },
      target: { type: 'string', description: "Unified textual target (B 档): '#channel', 'dm:@handle', or '<base>:<message-short-id>' (thread read by short id)." },
      messageId: { type: 'string', description: 'Read exactly this message id within targetId.' },
      replyToMessageId: { type: 'string', description: 'Resolve the Thread for this root Message id first (idempotent ensure), then read from it.' },
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
      const addressed = await resolveAgentTarget(collab, binding.agentId, { targetId: args.targetId, target: args.target })
      let resolvedTargetId = addressed.targetId
      if (typeof args.replyToMessageId === 'string' && args.replyToMessageId !== '') {
        if (args.target !== undefined) {
          throw new Error('target text and replyToMessageId are mutually exclusive; use one of them')
        }
        const thread = await collab.createThread(args.replyToMessageId, binding.agentId)
        resolvedTargetId = thread.id
      }
      const messages = args.messageId === undefined
        ? await collab.readMessages(binding.agentId, resolvedTargetId, afterSeq, limit)
        : [await collab.readMessage(binding.agentId, resolvedTargetId, args.messageId)]
      return { targetId: resolvedTargetId, target: addressed.display || undefined, messages }
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'message_send',
    description: 'Commit one text message to an exact collaboration target. Author identity is derived from the calling Agent and cannot be supplied by arguments.',
    parameters: {
      targetId: { type: 'string', description: 'Exact UUID target id. Mutually exclusive with target.' },
      target: { type: 'string', description: "Unified textual target (B 档): '#channel', 'dm:@handle', or '<base>:<message-short-id>' — the last resolves the Thread idempotently (tae EnsureThread), so a reply start needs no separate step." },
      text: { type: 'string', required: true, description: 'Text to commit.' },
      replyToMessageId: { type: 'string', description: 'Root Message id to reply in a Thread (A 档 idempotent ensure + deliver). Same-shape alternative to the <base>:<short> target text.' },
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
      presentationMeta: (_args, value) => ({
        kind: 'room-card',
        targetId: value.message.targetId,
        messageId: value.message.id,
        text: value.message.text,
      }),
    },
    async execute(args, exec) {
      exec.signal.throwIfAborted()
      const binding = await runtimes.bindingForExecution(requireAgent(exec.agent))
      // tae resolution order (application/target.go resolveThreadTarget):
      // composite reference → EnsureThread idempotent → regular send.
      const addressed = await resolveAgentTarget(collab, binding.agentId, { targetId: args.targetId, target: args.target })
      let targetId = addressed.targetId
      if (typeof args.replyToMessageId === 'string' && args.replyToMessageId !== '') {
        if (args.target !== undefined) {
          throw new Error('target text and replyToMessageId are mutually exclusive; use one of them')
        }
        const thread = await collab.createThread(args.replyToMessageId, binding.agentId)
        targetId = thread.id
      }
      return collab.sendMessage({
        targetId,
        authorId: binding.agentId,
        clientRequestId: String(exec.callId),
        text: args.text,
      })
    },
  }))

  agentCtx.tools.register(defineTool({
    name: 'task_create',
    description: 'Attach Task metadata to one committed top-level Channel or Direct message. Actor identity comes from the calling Agent. Convention (agreed 2026-08-19, "方案A"): the creator claims their fresh Task in the same call — create returns in_progress with self as assignee; the native pool semantics stay untouched underneath.',
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
      const task = await collab.createTask(args.messageId, binding.agentId)
      return collab.claimTask(task.messageId, binding.agentId)
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
