/** 工作台分类：笔记 topic。预置分类不能改。 */

import {
  isFriendLinkCategory,
  isMineCategory,
  phantasiCategoryParts,
} from '../constants'
import { normalizeNoteCategory } from '../notes/noteCategory'

const CANON_FRIEND_CATEGORY = '友情链接'
const CANON_MINE_CATEGORY = '我'

export interface WorkbenchCategoryRow {
  name: string
  catalogId: number | null
  noteCount: number
  locked: boolean
}

export function canonCategoryName(name: string): string {
  if (isFriendLinkCategory(name)) return CANON_FRIEND_CATEGORY
  if (isMineCategory(name)) return CANON_MINE_CATEGORY
  return name
}

export function isLockedCategory(name: string): boolean {
  return isFriendLinkCategory(name) || isMineCategory(name)
}

export function normalizeWorkbenchCategoryName(
  value: string | null | undefined,
): string | null {
  return normalizeNoteCategory(value)
}

export function renameCategoryPart(
  category: string | null | undefined,
  from: string,
  to: string,
): string | null {
  const parts = phantasiCategoryParts(category)
  let changed = false
  const next = parts.map((part) => {
    if (part !== from) return part
    changed = true
    return to
  })
  if (!changed) return category ?? null
  return next.join(', ')
}

export type AddCategoryPartResult =
  | { kind: 'next'; value: string }
  | { kind: 'skip'; reason: 'empty' | 'duplicate' | 'full' }

export function resolveAddCategoryPart(
  category: string | null | undefined,
  name: string,
): AddCategoryPartResult {
  const add = pickerCategoryName(name)
  if (!add) return { kind: 'skip', reason: 'empty' }
  const parts = phantasiCategoryParts(category)
  if (parts.some((part) => samePickerCategory(part, add))) {
    return { kind: 'skip', reason: 'duplicate' }
  }
  if (parts.length >= 2) return { kind: 'skip', reason: 'full' }
  return { kind: 'next', value: [...parts, add].join(', ') }
}

export function formatCategoryFullNotice(
  template: string,
  name: string,
): string {
  return template.replaceAll('{name}', name)
}

export function removeCategoryPart(
  category: string | null | undefined,
  name: string,
): string | null {
  const parts = phantasiCategoryParts(category)
  const next = parts.filter((part) => part !== name)
  if (next.length === parts.length) return category ?? null
  return next.length > 0 ? next.join(', ') : ''
}

export function collectWorkbenchCategoryRows(
  catalog: readonly { id: number; name: string }[],
  notes: readonly { topic?: string | null }[],
): WorkbenchCategoryRow[] {
  const catalogByName = new Map<string, number>()
  for (const item of catalog) {
    const name = canonCategoryName(item.name.trim())
    if (name && !catalogByName.has(name)) catalogByName.set(name, item.id)
  }
  const noteCounts = new Map<string, number>()
  for (const note of notes) {
    for (const part of phantasiCategoryParts(note.topic)) {
      const name = canonCategoryName(part)
      if (!name) continue
      noteCounts.set(name, (noteCounts.get(name) ?? 0) + 1)
    }
  }
  const names = new Set([...catalogByName.keys(), ...noteCounts.keys()])
  return [...names]
    .map((name) => ({
      name,
      catalogId: catalogByName.get(name) ?? null,
      noteCount: noteCounts.get(name) ?? 0,
      locked: isLockedCategory(name),
    }))
    .toSorted((a, b) => {
      if (a.locked !== b.locked) return a.locked ? 1 : -1
      return a.name.localeCompare(b.name, 'zh')
    })
}

export function noteMatchesCategory(
  note: { topic?: string | null },
  category: string,
): boolean {
  const want = canonCategoryName(category)
  if (!want) return false
  return phantasiCategoryParts(note.topic).some(
    (part) => canonCategoryName(part) === want,
  )
}

/** 笔记分类页不展示预置分类。 */
export function collectWorkbenchCategoryPageRows(
  catalog: readonly { id: number; name: string }[],
  notes: readonly { topic?: string | null }[],
): WorkbenchCategoryRow[] {
  return collectWorkbenchCategoryRows(catalog, notes).filter((row) => !row.locked)
}

export function canUseCategoryName(
  name: string,
  taken: readonly string[],
): boolean {
  if (!name) return false
  if (isLockedCategory(name)) return false
  return !taken.includes(name)
}

export function pickerCategoryName(value: string): string {
  const name = value.trim()
  if (!name) return ''
  return canonCategoryName(name)
}

export function samePickerCategory(a: string, b: string): boolean {
  const left = pickerCategoryName(a)
  const right = pickerCategoryName(b)
  return !!left && left === right
}
