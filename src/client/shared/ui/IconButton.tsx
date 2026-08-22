import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { Tooltip, type TooltipSide } from '@deepseek-ai/dsh-client-ui-primitives'
import { classNames } from '../class-names.ts'
import css from './IconButton.module.css'

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  label: string
  icon: ReactNode
  tooltip?: boolean | undefined
  tooltipSide?: TooltipSide | undefined
  selected?: boolean | undefined
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton({
  label,
  icon,
  tooltip = true,
  tooltipSide = 'bottom',
  selected,
  className,
  type = 'button',
  ...props
}, ref) {
  const button = (
    <button
      {...props}
      ref={ref}
      type={type}
      className={classNames(css.button, className)}
      aria-label={label}
      aria-pressed={selected}
      data-selected={selected === true ? 'true' : undefined}
    >
      {icon}
    </button>
  )

  if (!tooltip) return button
  return <Tooltip label={label} side={tooltipSide}>{button}</Tooltip>
})
