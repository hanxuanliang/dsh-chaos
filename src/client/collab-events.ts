/**
 * SSE client for GET /dsh-chaos/events (src/remote.ts serveCollabEvents).
 * Frames are invalidation-only ChangeEvents (`event: change`, `id: <seq>`) —
 * payload rereads stay on the RPC channel; `event: resync_required` means the
 * replay cursor fell out of the retention window and the store must do a full
 * snapshot reload. EventSource itself carries Last-Event-ID across automatic
 * reconnects, and the brief requires the connection to stay up while the panel
 * is closed (a later milestone reads unread badges off this feed).
 */
import type { NativeChangeEvent } from '../native.ts'

export type CollabConnectionState = 'live' | 'down'

export interface CollabEventsHandlers {
  onChange(change: NativeChangeEvent): void
  onResyncRequired(): void
  onConnection(state: CollabConnectionState): void
}

export class CollabEvents {
  private source: EventSource | undefined

  constructor(private readonly handlers: CollabEventsHandlers) {}

  /** (Re)connect from a change cursor; the previous socket is dropped first. */
  connect(cursor: string): void {
    this.disconnect()
    const source = new EventSource(`/dsh-chaos/events?cursor=${encodeURIComponent(cursor)}`)
    this.source = source
    source.onopen = () => { this.handlers.onConnection('live') }
    // Network drops land here too; EventSource keeps retrying on its own and
    // resumes from the last delivered event id, so 'down' is only a UI state.
    source.onerror = () => { this.handlers.onConnection('down') }
    source.addEventListener('change', (raw) => {
      const frame = raw as MessageEvent<string>
      try {
        this.handlers.onChange(JSON.parse(frame.data) as NativeChangeEvent)
      } catch {
        // A malformed frame is not a protocol failure; ignore it and let the
        // stream continue from its own cursor.
      }
    })
    source.addEventListener('resync_required', () => {
      // The server closes the socket after this frame; reconnecting with a
      // fresh post-reload cursor happens in the store, so stop the stale
      // socket from auto-resuming into another resync loop.
      this.disconnect()
      this.handlers.onResyncRequired()
    })
  }

  disconnect(): void {
    this.source?.close()
    this.source = undefined
  }
}
