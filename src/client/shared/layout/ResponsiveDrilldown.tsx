import { useEffect, useLayoutEffect, useRef, useState, type JSX, type ReactNode, type RefObject } from 'react'
import css from './ResponsiveDrilldown.module.css'

export function ResponsiveDrilldown({ desktop, list, detail, detailOpen, returnFocusRef, breakpoint = 700 }: {
  desktop: ReactNode
  list: ReactNode
  detail: ReactNode
  detailOpen: boolean
  returnFocusRef?: RefObject<HTMLElement | null> | undefined
  breakpoint?: number | undefined
}): JSX.Element {
  const rootRef = useRef<HTMLDivElement>(null)
  const [narrow, setNarrow] = useState(false)
  const previousOpen = useRef(detailOpen)

  useLayoutEffect(() => {
    const root = rootRef.current
    if (root === null) return
    const update = (width: number): void => { setNarrow(width <= breakpoint) }
    update(root.getBoundingClientRect().width)
    const observer = new ResizeObserver(entries => {
      const entry = entries[0]
      if (entry !== undefined) update(entry.contentRect.width)
    })
    observer.observe(root)
    return () => { observer.disconnect() }
  }, [breakpoint])

  useEffect(() => {
    if (narrow && previousOpen.current && !detailOpen) returnFocusRef?.current?.focus()
    previousOpen.current = detailOpen
  }, [detailOpen, narrow, returnFocusRef])

  return (
    <div ref={rootRef} className={css.root} data-narrow={narrow ? 'true' : undefined}>
      {narrow ? (detailOpen ? detail : list) : desktop}
    </div>
  )
}
