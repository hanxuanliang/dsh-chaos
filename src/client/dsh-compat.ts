/**
 * Compile-time boundary against the public DSH browser contracts.
 *
 * @deepseek-ai/dsh-client-locale is a host-internal package that is not
 * linked into this dependency graph, so the locale service the host injects
 * as ctx.locale is mirrored structurally here instead of imported.
 */
import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type { ClientContext } from '@deepseek-ai/dsh-client-runtime/client'
import type {} from '@deepseek-ai/dsh-client-ui-settings/client'
import type { ChaosKey, ChaosTranslate } from './locales.ts'

declare module '@deepseek-ai/dsh-client-ui-slots' {
  interface LocaleNamespaceMap {
    chaos: ChaosKey
  }
}

/** Subset of the host LocaleRuntime surface consumed by this plugin. */
export interface ChaosLocaleService {
  /** Register both dictionaries of one namespace; returns the disposer. */
  register(namespace: 'chaos', dictionaries: { zh: Record<string, string>; en: Record<string, string> }): () => void
  /** Bind the chaos namespace to a translate function reading the active locale. */
  bind(namespace: 'chaos'): ChaosTranslate
  /** Subscribe to active-locale changes (used by DOM-injected surfaces outside React). */
  subscribe(listener: () => void): () => void
  /** Read the active locale id (e.g. 'zh' | 'en'). */
  getSnapshot(): { active: string }
}

/**
 * Read-only subset of the host sessions service (`ctx.sessions`). Accessed
 * defensively (not declared in `inject`) following the dsh-mnemon precedent,
 * so a host build without the service only degrades the composer-block seat.
 */
export interface ChaosSessionsService {
  list: {
    getSnapshot(): { current: string | undefined }
    subscribe(listener: () => void): () => void
  }
}

/**
 * Composer-block face of the host conversation service (`ctx.conversation`):
 * the official "one way another plugin stops a session's input" (ui-conversation
 * input/blocks.ts). Raising a block renders the native composer inert with our
 * localized placeholder; clearing restores it.
 */
export interface ChaosConversationService {
  blocks: {
    set(sessionId: string, block: { reason: string } | undefined): void
  }
}

/** DSH 0.1.0-rc.7 client context plus the injected feature services. */
export type ChaosClientContext = ClientContext & {
  connection: ConnectionHandle
  locale: ChaosLocaleService
}
