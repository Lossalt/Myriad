/** 旧 10 key 的 i18n 字段。新主题直接显示名字。 */
export type TopicNameKey =
  | 'topicEngineering'
  | 'topicSystems'
  | 'topicAi'
  | 'topicProduct'
  | 'topicWriting'
  | 'topicTools'
  | 'topicCulture'
  | 'topicSecurity'
  | 'topicOss'
  | 'topicHardware'

interface LeftoverTopicDef {
  key: string
  nameKey: TopicNameKey
  hue: string
}

/** 库里还在的旧英文 key，只用来翻译和上色，不再当写入白名单。 */
const LEFTOVER_TOPIC_DEFS: readonly LeftoverTopicDef[] = [
  { key: 'engineering', nameKey: 'topicEngineering', hue: '#6366f1' },
  { key: 'systems', nameKey: 'topicSystems', hue: '#0ea5e9' },
  { key: 'ai', nameKey: 'topicAi', hue: '#8b5cf6' },
  { key: 'product', nameKey: 'topicProduct', hue: '#f97316' },
  { key: 'writing', nameKey: 'topicWriting', hue: '#d97706' },
  { key: 'tools', nameKey: 'topicTools', hue: '#14b8a6' },
  { key: 'culture', nameKey: 'topicCulture', hue: '#ec4899' },
  { key: 'security', nameKey: 'topicSecurity', hue: '#ef4444' },
  { key: 'oss', nameKey: 'topicOss', hue: '#22c55e' },
  { key: 'hardware', nameKey: 'topicHardware', hue: '#64748b' },
]

const LEFTOVER_BY_KEY = new Map(LEFTOVER_TOPIC_DEFS.map((d) => [d.key, d]))

const TOPIC_HUES = [210, 160, 25, 280, 340, 190, 45, 310] as const

export function topicNameKey(key: string): TopicNameKey | null {
  return LEFTOVER_BY_KEY.get(key)?.nameKey ?? null
}

export function topicDisplayName(
  topic: { key: string; nameKey?: TopicNameKey | null },
  labels: Partial<Record<TopicNameKey, string>>,
): string {
  const nameKey = topic.nameKey ?? topicNameKey(topic.key)
  if (nameKey) {
    const value = labels[nameKey]
    if (typeof value === 'string' && value.length > 0) return value
  }
  return topic.key
}

function hashTopicHue(name: string): string {
  let hash = 0
  for (const ch of name) {
    hash = (Math.imul(hash, 31) + (ch.codePointAt(0) ?? 0)) >>> 0
  }
  const hue = TOPIC_HUES[hash % TOPIC_HUES.length] ?? 210
  return `hsl(${hue} 42% 46%)`
}

export function topicHue(key: string): string {
  return LEFTOVER_BY_KEY.get(key)?.hue ?? hashTopicHue(key)
}
