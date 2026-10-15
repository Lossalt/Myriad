export function noteScheduleLabel(
  ms: number | null | undefined,
  locale: string,
): string {
  if (ms == null || !Number.isFinite(ms)) return ''
  return new Date(ms).toLocaleString(locale, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function noteEditorStatus(
  input: {
    lastError?: string | null
    status: string
    scheduledAt?: number | null
    savedHint?: boolean
  },
  copy: {
    failed: string
    scheduled: string
    published: string
    draft: string
    saved: string
  },
  locale: string,
): string {
  const when = noteScheduleLabel(input.scheduledAt, locale)
  const status = input.lastError
    ? copy.failed
    : input.status === 'scheduled'
      ? when
        ? `${copy.scheduled} · ${when}`
        : copy.scheduled
      : input.status === 'published'
        ? copy.published
        : copy.draft
  return input.savedHint ? `${status} · ${copy.saved}` : status
}
