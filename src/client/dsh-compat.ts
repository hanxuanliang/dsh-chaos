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
}

/** DSH 0.1.0-rc.7 client context plus the injected feature services. */
export type ChaosClientContext = ClientContext & {
  connection: ConnectionHandle
  locale: ChaosLocaleService
}
