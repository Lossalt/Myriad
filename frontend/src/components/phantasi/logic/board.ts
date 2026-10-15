export type WorkbenchPane =
  | 'home'
  | 'notes'
  | 'comments'
  | 'media'
  | 'notesIo'
  | 'noteCategories'
  | 'reviews'
  | 'blog'

export const NOTE_TRANSFER_KINDS = [
  'wordpress',
  'halo',
  'typecho',
  'markdown',
] as const

export type NoteTransferKind = (typeof NOTE_TRANSFER_KINDS)[number]
