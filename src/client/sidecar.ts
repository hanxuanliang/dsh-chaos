/** Optional better-sidebar surface. Isolation may not have the plugin. */

export interface SidecarTabProps {
  visible: boolean
}

export interface SidecarTabDescriptor {
  id: string
  title: string
  order?: number
  single?: boolean
  component: (props: SidecarTabProps) => unknown
}

export interface BetterSidebarLite {
  registerTab(descriptor: SidecarTabDescriptor): () => void
}

export function betterSidebarOf(ctx: unknown): BetterSidebarLite | undefined {
  try {
    const bag = ctx as { betterSidebar?: BetterSidebarLite; get?: (name: string) => unknown }
    const service = bag.betterSidebar ?? bag.get?.('betterSidebar')
    if (service === undefined || service === null || typeof (service as BetterSidebarLite).registerTab !== 'function') {
      return undefined
    }
    return service as BetterSidebarLite
  } catch {
    return undefined
  }
}
