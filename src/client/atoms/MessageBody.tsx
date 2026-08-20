/**
 * MessageBody — plocal §4 markdown 正文原子 (react-markdown gfm+breaks, 虚拟
 * components 渲到 token, claim clamp 344/320 + Show more, @mention 内文
 * 高亮的 rehype 辨别行家手, code pre+a 不触)。 原 MessageStream 内嵌单件。
 */
import { isValidElement, useLayoutEffect, useMemo, useRef, useEffect, useState, type ReactNode } from 'react'
import { IconCheckOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import Markdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkBreaks from 'remark-breaks'
import type { ChaosTranslate } from '../locales.ts'
import css from './MessageBody.module.css'
import { IconCopy } from './DomainIcons.tsx'

const CLAMP_TRIGGER_PX = 344
const REMARK_PLUGINS = [remarkGfm, remarkBreaks]
/** '@' at a word boundary: after start or a non-word char (CJK separators allowed). */
const MENTION = /(^|[^\p{L}\p{N}_@])@([^\s@]+)(?=$|[^\p{L}\p{N}_])/gu

interface HastLikeNode {
  type?: string
  value?: string
  tagName?: string
  properties?: Record<string, unknown>
  children?: HastLikeNode[]
}

/**
 * The leading guard char stays plain text; only the `@token` segment becomes a
 * highlighted span (a leading space/CJK char must not inherit the accent).
 * Returns null when nothing matches so untouched subtrees keep their identity.
 */
function splitMentionText(value: string, names: ReadonlySet<string>): HastLikeNode[] | null {
  const out: HastLikeNode[] = []
  let last = 0
  let hit = false
  for (const match of value.matchAll(MENTION)) {
    const at = match.index
    const lead = match[1] ?? ''
    const token = match[2] ?? ''
    if (!names.has(token.toLowerCase())) continue
    if (at + lead.length > last) out.push({ type: 'text', value: value.slice(last, at + lead.length) })
    out.push({
      type: 'element',
      tagName: 'span',
      properties: { className: [css.mention] },
      children: [{ type: 'text', value: `@${token}` }],
    })
    last = at + match[0].length
    hit = true
  }
  if (!hit) return null
  if (last < value.length) out.push({ type: 'text', value: value.slice(last) })
  return out
}

/**
 * @mention highlighting at the hast text-node level (react-markdown v10 has no
 * components.text hook): split text nodes carrying a known `@name`, skip
 * a/code/pre subtrees — code spans and fenced blocks can never highlight.
 * Zero new dependencies: a rehype plugin is just a tree transform function.
 */
function makeMentionRehype(names: ReadonlySet<string>): () => (tree: HastLikeNode) => void {
  const SKIP = new Set(['a', 'code', 'pre'])
  const walk = (node: HastLikeNode): void => {
    if (node.type === 'element' && node.tagName !== undefined && SKIP.has(node.tagName)) return
    if (node.children === undefined) return
    const next: HastLikeNode[] = []
    let changed = false
    for (const child of node.children) {
      const split = child.type === 'text' && child.value !== undefined ? splitMentionText(child.value, names) : null
      if (split !== null) {
        next.push(...split)
        changed = true
        continue
      }
      next.push(child)
      walk(child)
    }
    if (changed) node.children = next
  }
  return () => (tree) => { walk(tree) }
}

/** Plain-text content of a rendered subtree (for the code-block Copy button). */
function textOf(node: ReactNode): string {
  if (node === null || node === undefined || typeof node === 'boolean') return ''
  if (typeof node === 'string') return node
  if (typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(textOf).join('')
  if (isValidElement(node)) return textOf((node.props as { children?: ReactNode }).children)
  return ''
}

/** Fenced block shell: dark slab + hover-revealed 28px copy button (→ ✓ for 1.2s). */
function MarkdownPre({ t, children }: { t: ChaosTranslate; children?: ReactNode }): JSX.Element {
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<number | undefined>(undefined)
  useEffect(() => () => { window.clearTimeout(timerRef.current) }, [])
  const label = t(copied ? 'stream.copied' : 'stream.copy')
  return (
    <div className={css.preShell}>
      <pre className={css.mdPre}>{children}</pre>
      <button
        type="button"
        className={css.codeCopy}
        aria-label={label}
        title={label}
        onClick={() => {
          navigator.clipboard.writeText(textOf(children).replace(/\n$/, '')).then(() => {
            setCopied(true)
            window.clearTimeout(timerRef.current)
            timerRef.current = window.setTimeout(() => { setCopied(false) }, 1200)
          }, () => {
            // Clipboard unavailable (permissions / insecure context) — inert.
          })
        }}
      >
        {copied
          ? <IconCheckOutline16 />
          : <IconCopy />}
      </button>
    </div>
  )
}

/** plocal MessageBody's components map (truth §4), recolored to host tokens in CSS. */
function buildComponents(t: ChaosTranslate): Components {
  return {
    p: (props) => <p className={css.mdP}>{props.children}</p>,
    a: (props) => (
      <a className={css.mdLink} href={props.href} target="_blank" rel="noreferrer noopener">{props.children}</a>
    ),
    // Inline pill + fenced body share this element; .preShell resets the pill.
    code: (props) => (
      <code className={props.className === undefined ? css.mdCode : `${css.mdCode} ${props.className}`}>{props.children}</code>
    ),
    pre: (props) => <MarkdownPre t={t}>{props.children}</MarkdownPre>,
    // No external image fetching in the panel: render images as plain links.
    img: (props) => {
      const href = props.src ?? ''
      const alt = props.alt ?? ''
      return (
        <a className={css.mdLink} href={href} target="_blank" rel="noreferrer noopener">{alt !== '' ? alt : href}</a>
      )
    },
    h1: (props) => <h1 className={css.mdH1}>{props.children}</h1>,
    h2: (props) => <h2 className={css.mdH2}>{props.children}</h2>,
    h3: (props) => <h3 className={css.mdH3}>{props.children}</h3>,
    h4: (props) => <h4 className={css.mdH4}>{props.children}</h4>,
    ul: (props) => <ul className={css.mdUl}>{props.children}</ul>,
    ol: (props) => <ol className={css.mdOl}>{props.children}</ol>,
    li: (props) => <li className={css.mdLi}>{props.children}</li>,
    blockquote: (props) => <blockquote className={css.mdBlockquote}>{props.children}</blockquote>,
    hr: () => <hr className={css.mdHr} />,
    table: (props) => <div className={css.tableWrap}><table className={css.mdTable}>{props.children}</table></div>,
    th: (props) => <th className={css.mdTh} style={props.style}>{props.children}</th>,
    td: (props) => <td className={css.mdTd} style={props.style}>{props.children}</td>,
  }
}

/** Markdown body + plocal clamp (344 trigger / 320 cap / bottom fade / Show more). */
export function MessageBody({ t, text, names }: { t: ChaosTranslate; text: string; names: ReadonlySet<string> }): JSX.Element {
  const bodyRef = useRef<HTMLDivElement | null>(null)
  const [oversize, setOversize] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const mentionRehype = useMemo(() => makeMentionRehype(names), [names])
  const components = useMemo(() => buildComponents(t), [t])

  useLayoutEffect(() => {
    const el = bodyRef.current
    if (el !== null) setOversize(el.scrollHeight > CLAMP_TRIGGER_PX)
  }, [text])

  const clamped = oversize && !expanded
  return (
    <>
      <div ref={bodyRef} className={css.msgText} data-clamped={clamped || undefined}>
        <Markdown remarkPlugins={REMARK_PLUGINS} rehypePlugins={[mentionRehype]} components={components}>
          {text}
        </Markdown>
        {clamped && <div className={css.clampFade} aria-hidden="true" />}
      </div>
      {oversize && (
        <button type="button" className={css.clampToggle} onClick={() => { setExpanded(value => !value) }}>
          {t(expanded ? 'stream.collapse' : 'stream.showMore')}
        </button>
      )}
    </>
  )
}

