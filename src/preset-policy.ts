const SUPPORTED_AGENT_PRESET_IDS = ['standard', 'code', 'cordis'] as const

export type SupportedAgentPresetId = typeof SUPPORTED_AGENT_PRESET_IDS[number]

const supportedPresetIds = new Set<string>(SUPPORTED_AGENT_PRESET_IDS)

export function isSupportedAgentPreset(id: string): id is SupportedAgentPresetId {
  return supportedPresetIds.has(id)
}

export function requireSupportedAgentPreset(id: string): SupportedAgentPresetId {
  if (!isSupportedAgentPreset(id)) {
    throw new Error(
      `[invalid_argument] preset must be one of: ${SUPPORTED_AGENT_PRESET_IDS.join(', ')}`,
    )
  }
  return id
}
