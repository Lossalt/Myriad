import type { PhantasiSource } from '../../../types/phantasi'

const MS_PER_DAY = 86_400_000

export const NOW = Date.UTC(2025, 5, 15)

export function daysAgo(days: number, now: number = NOW): number {
  return now - days * MS_PER_DAY
}

export function makeSource(over: Partial<PhantasiSource> = {}): PhantasiSource {
  return {
    id: 1,
    user_id: 1,
    name: '示例源',
    url: 'https://example.com/feed.xml',
    feed_type: 'rss',
    source_type: 'rss',
    category: null,
    icon: null,
    description: null,
    site_url: 'https://example.com',
    update_interval: 3600,
    last_fetched_at: daysAgo(1),
    last_success_at: daysAgo(1),
    last_error: null,
    error_count: 0,
    enabled: true,
    item_count: 100,
    unread_count: 0,
    card_size: null,
    theme_color: '#f97316',
    sort_order: null,
    ai_style_tags: null,
    rsshub_route: null,
    admin_only: false,
    created_at: daysAgo(400),
    recent_items: [],
    ...over,
  }
}
