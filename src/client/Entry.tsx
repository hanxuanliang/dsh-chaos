import type { InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import type { ChaosInjected } from './surface.tsx'
import css from './surface.module.css'

type EntryProps = PropsRuntime<'sidebar.footer.action'> & InjectFace<ChaosInjected>

function ActivityGlyph(): React.JSX.Element {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M8 2a4 4 0 0 0-4 4v2.4L2.8 10h10.4L12 8.4V6a4 4 0 0 0-4-4ZM6.5 12a1.5 1.5 0 0 0 3 0"
        stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round"
      />
    </svg>
  )
}

/** Activity entry at the sidebar foot: full row when wide, icon when rail. */
export function ActivityEntry(props: EntryProps): React.JSX.Element {
  const wide = (props as unknown as { wide?: boolean }).wide !== false
  return (
    <button
      type="button"
      className={wide ? css.entryRow : css.entryIcon}
      aria-label="Activity"
      title="Activity"
      onClick={() => { props.openActivity() }}
    >
      <ActivityGlyph />
      {wide ? <span className={css.entryLabel}>Activity</span> : null}
    </button>
  )
}
