/**
 * Open-state store for the collab overlay panel. Deliberately identical in
 * shape to the dsh-mnemon workspace controller (subscribe/getSnapshot +
 * open/close/toggle) so the DOM mount layer stays interchangeable.
 */
export interface CollabPanelSnapshot {
  readonly open: boolean
}

export class CollabPanelController {
  private snapshot: CollabPanelSnapshot = { open: false }
  private readonly listeners = new Set<() => void>()

  readonly getSnapshot = (): CollabPanelSnapshot => this.snapshot

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  open(): void { this.set(true) }
  close(): void { this.set(false) }
  toggle(): void { this.set(!this.snapshot.open) }

  private set(open: boolean): void {
    if (this.snapshot.open === open) return
    this.snapshot = { open }
    for (const listener of this.listeners) listener()
  }
}
