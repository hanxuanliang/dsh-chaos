/** The immutable collaboration handle is the Agent's trimmed initial name. */
export function initialAgentHandle(name: string): string {
  return name.trim()
}

/** Handles differing only by case would be ambiguous in mention search. */
export function hasAgentHandle(handle: string, existingHandles: readonly string[]): boolean {
  const normalized = handle.toLocaleLowerCase()
  return existingHandles.some(existing => existing.toLocaleLowerCase() === normalized)
}
