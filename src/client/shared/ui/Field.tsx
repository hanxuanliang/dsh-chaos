import { cloneElement, isValidElement, useId, type JSX, type ReactElement, type ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './Field.module.css'

export function Field({ label, hint, error, required, children, className }: {
  label: ReactNode
  hint?: ReactNode
  error?: ReactNode
  required?: boolean | undefined
  children: ReactElement<{ id?: string, 'aria-describedby'?: string, 'aria-invalid'?: boolean }>
  className?: string | undefined
}): JSX.Element {
  const generatedId = useId()
  const controlId = children.props.id ?? `${generatedId}-control`
  const descriptionId = hint !== undefined || error !== undefined ? `${generatedId}-description` : undefined
  const control = isValidElement(children)
    ? cloneElement(children, {
        id: controlId,
        ...(descriptionId === undefined ? {} : { 'aria-describedby': descriptionId }),
        ...(error === undefined ? {} : { 'aria-invalid': true }),
      })
    : children

  return (
    <div className={classNames(css.field, className)} data-invalid={error === undefined ? undefined : 'true'}>
      <label className={css.label} htmlFor={controlId}>
        {label}
        {required === true && <span className={css.required} aria-hidden="true">*</span>}
      </label>
      {control}
      {descriptionId !== undefined && (
        <div id={descriptionId} className={error === undefined ? css.hint : css.error}>
          {error ?? hint}
        </div>
      )}
    </div>
  )
}
