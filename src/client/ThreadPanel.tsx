import { useEffect, useRef, useState, type FormEvent } from 'react'
import type { NativeMessage, NativeTarget } from '../native.ts'
import css from './ChaosPanel.module.css'

/**
 * Right-rail Thread panel: the main conversation stays put while the thread
 * is read and answered here. Follow/unfollow only changes attention and the
 * left-nav nesting — an open panel keeps reading and receiving SSE refreshes
 * (controller-owned) even after unfollow.
 */
export function ThreadPanel({
  thread,
  parentName,
  messages,
  names,
  followed,
  pending,
  onFollow,
  onUnfollow,
  onClose,
  onSend,
}: {
  thread: NativeTarget
  parentName: string
  messages: readonly NativeMessage[]
  names: ReadonlyMap<string, string>
  followed: boolean
  pending: boolean
  onFollow: () => void
  onUnfollow: () => void
  onClose: () => void
  onSend: (text: string) => Promise<void>
}) {
  const [draft, setDraft] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const node = scrollRef.current
    if (node !== null) node.scrollTop = node.scrollHeight
  }, [messages.length, thread.id])

  const submit = (event: FormEvent): void => {
    event.preventDefault()
    const text = draft.trim()
    if (text === '') return
    void onSend(text).then(() => { setDraft('') })
  }

  const rootSuffix = thread.rootMessageId?.slice(-5) ?? thread.id.slice(-5)

  return (
    <section className={css.threadPanel} aria-label={`Thread ${parentName} · ${rootSuffix}`}>
      <header className={css.threadPanelHeader}>
        <div className={css.threadPanelTitle}>
          <span className={css.targetKind}>Thread</span>
          <strong>{parentName} · {rootSuffix}</strong>
        </div>
        <div className={css.headerActions}>
          <button
            type="button"
            className={followed ? css.secondaryButton : css.primaryButton}
            disabled={pending}
            onClick={followed ? onUnfollow : onFollow}
          >
            {followed ? '取消关注' : '关注'}
          </button>
          <button type="button" className={css.iconButton} aria-label="关闭 Thread 面板" onClick={onClose}>×</button>
        </div>
      </header>
      <div ref={scrollRef} className={css.threadPanelMessages}>
        {messages.length === 0 && <p className={css.empty}>Thread 里还没有回复。</p>}
        {messages.map(message => (
          <div key={message.id} className={css.messageRow}>
            <div className={css.messageRowHeader}>
              <strong>{names.get(message.authorId) ?? message.authorId}</strong>
              <time>{new Date(message.createdAtMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
            </div>
            <p className={css.messageText}>{message.text}</p>
          </div>
        ))}
      </div>
      <form onSubmit={submit} className={css.threadPanelComposer}>
        <input
          value={draft}
          onChange={event => { setDraft(event.target.value) }}
          placeholder="回复 Thread"
          disabled={pending}
          aria-label="回复 Thread"
        />
        <button className={css.primaryButton} disabled={pending || draft.trim() === ''}>发送</button>
      </form>
    </section>
  )
}
