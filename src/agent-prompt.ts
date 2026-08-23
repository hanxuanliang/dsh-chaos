import type { Context } from '@deepseek-ai/cordis'
import type { NativeAgentProfile } from './native.ts'

const IDENTITY_SECTION = 'chaos:collaboration-identity'

function renderList(title: string, values: readonly string[]): string[] {
  return values.length === 0
    ? []
    : ['', `### ${title}`, ...values.map(value => `- ${value}`)]
}

export function renderAgentIdentityPrompt(profile: NativeAgentProfile): string {
  return [
    '## Collaboration identity',
    `You are ${profile.actor.displayName} (@${profile.actor.handle}), a stable Chaos collaboration Agent.`,
    '',
    '### Charter',
    profile.charter.summary,
    ...renderList('Capabilities', profile.charter.capabilities),
    ...renderList('Constraints', profile.charter.constraints),
    '',
    '### Workspace and continuity',
    `Your persistent, agent-owned workspace is ${profile.workspacePath}. Its layout is intentionally unspecified; organize it as the work requires.`,
    'Keep MEMORY.md as the recovery entry point. When durable material or organization must be rediscovered by a future session, leave a concise pointer there.',
    '',
    '### Collaboration rules',
    'When a Collab inbox notice says messages are pending, call message_check before replying.',
    'Use human-readable @handles in conversation and never expose internal actor UUIDs in prose.',
    'Current Messages, Tasks, Memberships, Delivery state, system signals, and executable evidence override conflicting local memory.',
  ].join('\n')
}

/** Register one live Profile projection in the unpublished DSH Agent scope. */
export function installAgentIdentityPrompt(
  agentCtx: Context,
  profile: () => NativeAgentProfile,
): void {
  agentCtx.systemPrompt.section({
    name: IDENTITY_SECTION,
    order: 20,
    text: () => renderAgentIdentityPrompt(profile()),
  })
}
