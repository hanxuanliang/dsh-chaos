/** Cross-surface navigation from Settings into the mounted Collab workspace. */
export const CHAOS_NAVIGATE_CHANNEL_EVENT = 'dsh-chaos-navigate-channel'

export function navigateToCollabChannel(targetId: string): void {
  // settings.section has no public close service in rc.7. The host's Settings
  // dialog does expose its standard Escape contract; use it before opening the
  // workspace so the new surface is not hidden behind the old modal.
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }))
  window.requestAnimationFrame(() => {
    document.dispatchEvent(new CustomEvent<string>(CHAOS_NAVIGATE_CHANNEL_EVENT, { detail: targetId }))
  })
}
