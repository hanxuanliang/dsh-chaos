import { useEffect, useRef, useState, type JSX, type ReactNode, type RefObject } from 'react'

function isNarrow(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(max-width: 700px)').matches
}

export function ResponsiveDrilldown({ desktop, list, detail, detailOpen, returnFocusRef }: {
  desktop: ReactNode
  list: ReactNode
  detail: ReactNode
  detailOpen: boolean
  returnFocusRef?: RefObject<HTMLElement | null> | undefined
}): JSX.Element {
  const [narrow, setNarrow] = useState(isNarrow)
  const previousOpen = useRef(detailOpen)

  useEffect(() => {
    const query = window.matchMedia('(max-width: 700px)')
    const update = (): void => { setNarrow(query.matches) }
    update()
    query.addEventListener('change', update)
    return () => { query.removeEventListener('change', update) }
  }, [])

  useEffect(() => {
    if (narrow && previousOpen.current && !detailOpen) returnFocusRef?.current?.focus()
    previousOpen.current = detailOpen
  }, [detailOpen, narrow, returnFocusRef])

  if (!narrow) return <>{desktop}</>
  return <>{detailOpen ? detail : list}</>
}
