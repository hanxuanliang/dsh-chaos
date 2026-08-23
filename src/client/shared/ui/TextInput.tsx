import { Input } from '@deepseek-ai/dsh-client-ui-primitives'
import type { ComponentProps, JSX } from 'react'
import { classNames } from '../class-names.ts'
import css from './TextInput.module.css'

export type TextInputProps = ComponentProps<typeof Input>

/** DSH Input with the shared Chaos field geometry and blue focus treatment. */
export function TextInput({ className, ...props }: TextInputProps): JSX.Element {
  return (
    <Input
      {...props}
      className={classNames(css.input, className)}
      data-chaos-text-input="true"
    />
  )
}
