/** RootCard — thread 顶部原消息卡: avatar + 作者名 + MessageBody 全文;
 * 右上放 panel 定调不高(可点 by onJump; onJump 未给时为 div); rootMessage
 * 缺失时显示 missing 行。 由 ThreadPanel 与 Activity dock 共用钓出。 */
import type { JSX } from 'react'
import type { NativeActor, NativeMessage } from '../../../native.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import type { AvatarSeed } from '../../shared/avatar.ts'
import { MessageBody } from '../messages/MessageBody.tsx'
import css from './RootCard.module.css'

export interface RootCardProps {
  t: ChaosTranslate
  /** 原生 root 消息; undefined=缓存轻便 → 显示 missing 文案 */
  rootMessage: NativeMessage | undefined
  rootAuthor: NativeActor | undefined
  /** 点击卡 → 跳父位 (panel 内流滚动到 root)。 未给 → 卡不可点。 */
  onJump?: (rootMessageId: string) => void
  /** AvatarChip seed; 通常=父消息作者 handle 首字符首字符下定 */
  seed: AvatarSeed
  /** markdown 高亮 mention 名单 */
  mentionNames: ReadonlySet<string>
}

export function RootCard({ t, rootMessage, rootAuthor, onJump, seed, mentionNames }: RootCardProps): JSX.Element {
  if (rootMessage === undefined) {
    return (
      <div className={css.root} data-plugin="dsh-chaos" data-missing="true">
        <span className={css.rootText}>{t('thread.rootMissing')}</span>
      </div>
    )
  }
  const inner = (
    <>
      <span className={css.rootHead}>
        <AvatarChip seed={seed} avatarUrl={rootAuthor?.avatarDataUrl} aria-hidden="true" />
        <span className={css.rootAuthor}>{rootAuthor?.displayName ?? rootMessage.authorId}</span>
      </span>
      <div className={css.rootText}>
        <MessageBody t={t} text={rootMessage.text} names={mentionNames} />
      </div>
    </>
  )
  if (onJump === undefined) {
    return <div className={css.root} data-plugin="dsh-chaos">{inner}</div>
  }
  return (
    <button type="button" className={`${css.root} ${css.rootLink}`} data-plugin="dsh-chaos" title={t('thread.rootJump')} onClick={() => { onJump(rootMessage.id) }}>
      {inner}
    </button>
  )
}
