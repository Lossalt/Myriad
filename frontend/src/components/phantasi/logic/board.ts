import type { PhantasiSource } from '../../../types/phantasi'
import {
  isFriendLinkCategory,
  isMineCategory,
  phantasiCategoryParts,
} from '../constants'

export type WorkbenchPane =
  | 'home'
  | 'notes'
  | 'comments'
  | 'media'
  | 'sources'
  | 'add'
  | 'topics'
  | 'rsshub'
  | 'notesIo'
  | 'feedsIo'
  | 'noteCategories'
  | 'sourceCategories'
  | 'reviews'

export const NOTE_TRANSFER_KINDS = [
  'wordpress',
  'halo',
  'typecho',
  'markdown',
] as const

export type NoteTransferKind = (typeof NOTE_TRANSFER_KINDS)[number]

export function sourceMatchesCategory(
  source: Pick<PhantasiSource, 'category'>,
  category: string,
): boolean {
  const parts = phantasiCategoryParts(source.category)
  if (isFriendLinkCategory(category)) return parts.some(isFriendLinkCategory)
  if (isMineCategory(category)) return parts.some(isMineCategory)
  return parts.includes(category)
}

export type SourceSortMode = 'smart' | 'update' | 'category' | 'pinyin'

export const WORKBENCH_PANES = [
  'home',
  'notes',
  'comments',
  'media',
  'sources',
  'add',
  'topics',
  'rsshub',
  'notesIo',
  'feedsIo',
  'noteCategories',
  'sourceCategories',
  'reviews',
] as const satisfies readonly WorkbenchPane[]

/** 旧深链 pane=list 并进订阅页；pane=add 进添加订阅。导入导出旧名并进两页。 */
export function resolveWorkbenchPane(value: string | null): WorkbenchPane {
  if (value === 'list') return 'sources'
  if (value === 'category' || value === 'categories') return 'noteCategories'
  if (value === 'sourceCategory') return 'sourceCategories'
  if (
    value === 'wordpress' ||
    value === 'halo' ||
    value === 'typecho' ||
    value === 'markdown' ||
    value === 'notesIo'
  ) {
    return 'notesIo'
  }
  if (value === 'pipack' || value === 'opml' || value === 'feedsIo') {
    return 'feedsIo'
  }
  if (value && (WORKBENCH_PANES as readonly string[]).includes(value)) {
    return value as WorkbenchPane
  }
  return 'home'
}
