export function draftKeyOf(input: {
  selectedTargetId?: string
  threadPanelId?: string
}): string {
  if (input.threadPanelId !== undefined) return `thread:${input.threadPanelId}`
  if (input.selectedTargetId !== undefined) return `channel:${input.selectedTargetId}`
  return 'session'
}

export function swapDraft(
  drafts: Readonly<Record<string, string>>,
  fromKey: string,
  toKey: string,
  live: string,
): { drafts: Readonly<Record<string, string>>; next: string } {
  if (fromKey === toKey) return { drafts, next: live }
  return {
    drafts: { ...drafts, [fromKey]: live },
    next: drafts[toKey] ?? '',
  }
}
