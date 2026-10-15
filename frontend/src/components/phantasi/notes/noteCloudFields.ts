import type { PhantasiNoteDoc } from '../../../types/phantasi'
import { mergeNoteField, mergeNoteText } from './noteMerge'

export interface NoteCloudFields {
  title: string
  contentMd: string
  topic: string | null
  cover: string | null
  publishedAt: number | null
  slug?: string | null
}

export function sameCloudFields(a: NoteCloudFields, b: NoteCloudFields): boolean {
  return (
    a.title === b.title &&
    a.contentMd === b.contentMd &&
    (a.topic ?? null) === (b.topic ?? null) &&
    (a.cover ?? null) === (b.cover ?? null) &&
    (a.publishedAt ?? null) === (b.publishedAt ?? null) &&
    (a.slug ?? null) === (b.slug ?? null)
  )
}

export function cloudFieldsOf(doc: PhantasiNoteDoc): NoteCloudFields {
  return {
    title: doc.title,
    contentMd: doc.content_md,
    topic: doc.topic ?? null,
    cover: doc.image ?? null,
    publishedAt: doc.published_at ?? null,
    slug: doc.slug?.trim() || null,
  }
}

/** 三路合并整份快照；远端改过的发布时间优先，否则保留本地未确认修改。 */
export function mergeCloudFields(
  base: NoteCloudFields,
  local: NoteCloudFields,
  remote: NoteCloudFields,
): NoteCloudFields {
  return {
    title: mergeNoteText(base.title, local.title, remote.title),
    contentMd: mergeNoteText(base.contentMd, local.contentMd, remote.contentMd),
    topic: mergeNoteField(base.topic, local.topic, remote.topic),
    cover: mergeNoteField(base.cover, local.cover, remote.cover),
    publishedAt: remote.publishedAt === base.publishedAt ? local.publishedAt : remote.publishedAt,
    slug: mergeNoteField(base.slug ?? null, local.slug ?? null, remote.slug ?? null),
  }
}
