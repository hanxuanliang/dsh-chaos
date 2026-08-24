import type { JSX } from 'react'
import { IconQuestionOutline14, Tooltip } from '@deepseek-ai/dsh-client-ui-primitives'
import css from './HelpHint.module.css'

/**
 * Standalone help affordance for controls that do not sit inside a `Field`
 * (e.g. the borderless circle-style identity block in the create form).
 * Same grammar as Field's inline help: question glyph, hover/focus tooltip.
 */
export function HelpHint({ label }: { label: string }): JSX.Element {
  return (
    <Tooltip label={label} side="top" maxWidth={280}>
      <button type="button" className={css.help} aria-label={label}>
        <IconQuestionOutline14 size={12} />
      </button>
    </Tooltip>
  )
}
