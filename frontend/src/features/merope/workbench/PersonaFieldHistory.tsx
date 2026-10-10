import type { StructuredPersona } from '../../../components/agent/onboarding/onboardingTypes'
import type { PersonaFieldHistoryEntry } from '../../../services/agent/agentApi'
import { useEffect, useState } from 'react'
import { joinList } from '../../../components/agent/onboarding/onboardingTypes'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { userFacingError } from '../../../utils/userFacingError'

type Field = keyof StructuredPersona

interface Props {
  /** The persona field, as her persona names it (`summary`, `likes`, …). */
  field: Field
  persona: StructuredPersona
  busy?: boolean
  /** Saves her persona with an old value put back: an ordinary edit. */
  onPersona: (persona: StructuredPersona) => void
  onClose: () => void
}

const LISTS: readonly Field[] = ['temperament', 'likes', 'drives']

function display(field: Field, value: unknown): string {
  if (LISTS.includes(field)) return joinList(value)
  return typeof value === 'string' ? value : ''
}

/** Her persona with `value` back in `field`, when it is a value of that field. */
function restored(
  persona: StructuredPersona,
  field: Field,
  value: unknown,
): StructuredPersona | null {
  if (LISTS.includes(field)) {
    return Array.isArray(value)
      ? {
          ...persona,
          [field]: value.filter(
            (item): item is string => typeof item === 'string',
          ),
        }
      : null
  }
  return typeof value === 'string' ? { ...persona, [field]: value } : null
}

/** What one field of her persona held before, newest first, each can be put back. */
export default function PersonaFieldHistory({
  field,
  persona,
  busy = false,
  onPersona,
  onClose,
}: Props) {
  const { t, locale } = useI18n()
  const m = t.merope
  const [entries, setEntries] = useState<PersonaFieldHistoryEntry[] | null>(
    null,
  )
  const [error, setError] = useState('')

  useEffect(() => {
    let live = true
    agentService
      .getPersonaHistory(field)
      .then((loaded) => {
        if (live) setEntries(loaded)
      })
      .catch((reason: unknown) => {
        if (live) setError(userFacingError(reason, m.personaHistoryFailed))
      })
    return () => {
      live = false
    }
  }, [field, m.personaHistoryFailed])

  const when = (at: string) => {
    const date = new Date(at)
    return Number.isNaN(date.getTime())
      ? at
      : date.toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
  }

  return (
    <div
      className="merope-persona-history"
      role="region"
      aria-label={m.personaHistory}
    >
      <div className="merope-persona-history__head">
        <span>{m.personaHistory}</span>
        <SettingsButton type="button" size="sm" onClick={onClose}>
          {m.personaHistoryClose}
        </SettingsButton>
      </div>
      {error ? (
        <p className="merope-motion-home__help" role="alert">
          {error}
        </p>
      ) : entries === null ? (
        <p className="merope-motion-home__help">{m.personaHistoryLoading}</p>
      ) : entries.length === 0 ? (
        <p className="merope-motion-home__help">{m.personaHistoryEmpty}</p>
      ) : (
        <ol className="merope-persona-history__list">
          {entries.map((entry) => (
            <li key={entry.id} className="merope-persona-history__item">
              <div className="merope-persona-history__copy">
                <small>
                  {m.personaHistoryReplacedAt.replace(
                    '{time}',
                    when(entry.replacedAt),
                  )}
                  {entry.origin === 'site'
                    ? ''
                    : ` · ${m.personaHistoryFromDevice}`}
                </small>
                <p>{display(field, entry.value) || '—'}</p>
              </div>
              <SettingsButton
                type="button"
                size="sm"
                disabled={busy || !restored(persona, field, entry.value)}
                onClick={() => {
                  const next = restored(persona, field, entry.value)
                  if (next) onPersona(next)
                  onClose()
                }}
              >
                {m.personaHistoryRestore}
              </SettingsButton>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
