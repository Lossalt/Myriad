export const BLOG_BUILD_STATUSES = [
  'idle',
  'queued',
  'running',
  'ok',
  'failed',
  'unavailable',
] as const

export type BlogBuildStatus = (typeof BLOG_BUILD_STATUSES)[number]

export interface BlogSettings {
  theme_id: string
  site_title: string
  description: string
  params: Record<string, unknown>
}

export interface BlogTheme {
  id: string
  name: string
  version?: string
  min_version?: string
  builtin: boolean
  installed_at?: string
}

export interface BlogHugo {
  available: boolean
  version?: string
}

export interface BlogBuild {
  status: BlogBuildStatus
  last_built_at?: string
  last_error?: string
  log_tail?: string
}

export interface BlogSnapshot {
  settings: BlogSettings
  themes: BlogTheme[]
  hugo: BlogHugo
  build: BlogBuild
}

export interface BlogThemeInstall {
  id: string
  name: string
  version?: string
  min_version?: string
  builtin: boolean
  installed_at?: string
}
