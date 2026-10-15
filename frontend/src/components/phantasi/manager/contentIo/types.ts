export interface ImportProgress {
  step: string
  current: number
  total: number
}

export type TransferNoteStatus = 'draft' | 'published'

export interface TransferNote {
  title: string
  content_md: string
  topic: string | null
  published_at: number | null
  status: TransferNoteStatus
}

export type { NoteTransferKind } from '../../logic/board'
