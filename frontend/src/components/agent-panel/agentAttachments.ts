export const AGENT_ATTACH_MAX_COUNT = 4
export const AGENT_ATTACH_MAX_BYTES = 8 * 1024 * 1024
export const AGENT_ATTACH_TEXT_CHARS = 8000
export const AGENT_ATTACH_MODEL_EDGE = 1024
const PREVIEW_EDGE = 384
export const AGENT_ATTACH_ACCEPT =
  'image/*,text/plain,text/markdown,text/csv,application/json,application/xml,text/xml,.txt,.md,.markdown,.csv,.json,.xml'

export interface AgentAttachment {
  id: string
  name: string
  mime: string
  size: number
  /** UI only; omitted from the request. */
  previewUrl?: string
  /**
   * What the model looks at: a JPEG data URL at most `AGENT_ATTACH_MODEL_EDGE`
   * on the long side. Sent with this message only, never kept in the chat.
   */
  modelImage?: string
  text?: string
}

export type AttachError = 'tooMany' | 'tooLarge' | 'unsupported'

const TEXT_TYPES = new Set([
  'text/plain',
  'text/markdown',
  'text/csv',
  'application/json',
  'application/xml',
  'text/xml',
])

export function isAttachableFile(file: File): boolean {
  if (file.type.startsWith('image/')) return true
  if (TEXT_TYPES.has(file.type)) return true
  return /\.(txt|md|markdown|csv|json|xml)$/i.test(file.name)
}

export function attachErrorFor(
  file: File,
  currentCount: number,
): AttachError | null {
  if (currentCount >= AGENT_ATTACH_MAX_COUNT) return 'tooMany'
  if (file.size > AGENT_ATTACH_MAX_BYTES) return 'tooLarge'
  if (!isAttachableFile(file)) return 'unsupported'
  return null
}

export async function fileToAttachment(file: File): Promise<AgentAttachment> {
  const id = `att_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  const mime = file.type || 'application/octet-stream'
  const base: AgentAttachment = {
    id,
    name: file.name,
    mime,
    size: file.size,
  }
  if (file.type.startsWith('image/')) {
    const previewUrl = await downscaledDataUrl(file, PREVIEW_EDGE, 'image/webp', 0.7)
    // JPEG encodes everywhere (Safari cannot encode WebP from a canvas).
    const modelImage = await downscaledDataUrl(
      file,
      AGENT_ATTACH_MODEL_EDGE,
      'image/jpeg',
      0.85,
    ).catch(() => undefined)
    return { ...base, previewUrl, ...(modelImage ? { modelImage } : {}) }
  }
  const raw = await file.text()
  const text = raw.slice(0, AGENT_ATTACH_TEXT_CHARS)
  return { ...base, text }
}

/** Decode a bounded bitmap, retain only the encoded copy, and release native pixels. */
async function downscaledDataUrl(
  file: File,
  edge: number,
  type: string,
  quality: number,
): Promise<string> {
  const bitmap = await createImageBitmap(file, { resizeWidth: edge, resizeQuality: 'high' })
  try {
    const scale = Math.min(1, edge / Math.max(bitmap.width, bitmap.height))
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(bitmap.width * scale))
    canvas.height = Math.max(1, Math.round(bitmap.height * scale))
    const context = canvas.getContext('2d')
    if (!context) throw new Error('Image preview unavailable')
    if (type === 'image/jpeg') {
      // No alpha in JPEG: transparent areas would turn black.
      context.fillStyle = '#fff'
      context.fillRect(0, 0, canvas.width, canvas.height)
    }
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height)
    const preview = canvas.toDataURL(type, quality)
    canvas.width = canvas.height = 0
    return preview
  } finally {
    bitmap.close()
  }
}

export function attachmentsForRequest(
  attachments: readonly AgentAttachment[],
): Array<{ name: string; mime: string; size: number; text?: string; image?: string }> {
  return attachments.map(({ name, mime, size, text, modelImage }) => ({
    name,
    mime,
    size,
    ...(text ? { text } : {}),
    ...(modelImage ? { image: modelImage } : {}),
  }))
}

/** The attachments a chat message keeps: the model-size image goes with the request only. */
export function attachmentsForDisplay(
  attachments: readonly AgentAttachment[],
): AgentAttachment[] {
  return attachments.map(({ modelImage: _modelImage, ...kept }) => kept)
}

export async function collectAttachments(
  files: Iterable<File>,
  current: readonly AgentAttachment[],
): Promise<{ attachments: AgentAttachment[]; error: AttachError | null }> {
  const next = Iterator.from(current).toArray()
  let error: AttachError | null = null
  for (const file of files) {
    const err = attachErrorFor(file, next.length)
    if (err) {
      error ??= err
      continue
    }
    next.push(await fileToAttachment(file))
  }
  return { attachments: next, error }
}
