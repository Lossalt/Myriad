import type { FontOption, LayoutOption, ThemeConfig, ThemeKey } from './types'

export const FONT_OPTIONS: FontOption[] = [
  {
    id: 'serif',
    family: '"Noto Serif SC", "Source Han Serif SC", "Songti SC", serif',
  },
  {
    id: 'sans',
    family: '"Noto Sans SC", "Source Han Sans SC", "PingFang SC", sans-serif',
  },
  {
    id: 'system',
    family: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  },
]

export const THEMES: Record<ThemeKey, ThemeConfig> = {
  light: {
    bg: 'bg-[#f8f5ec]',
    text: 'text-[#2c2c2c]',
    secondary: 'text-[#666]',
    border: 'phantasi-reader__line',
    surface: 'phantasi-reader__chip',
    surfaceSolid: 'phantasi-reader__chip is-solid',
    accent: '#b8860b',
    icon: 'L',
  },
  sepia: {
    bg: 'bg-[#f4ecd8]',
    text: 'text-[#3d3229]',
    secondary: 'text-[#6b5d4d]',
    border: 'phantasi-reader__line',
    surface: 'phantasi-reader__chip',
    surfaceSolid: 'phantasi-reader__chip is-solid',
    accent: '#8b6914',
    icon: 'S',
  },
  dark: {
    bg: 'bg-[#1a1a1a]',
    text: 'text-[#c9c9c9]',
    secondary: 'text-[#888]',
    border: 'phantasi-reader__line',
    surface: 'phantasi-reader__chip',
    surfaceSolid: 'phantasi-reader__chip is-solid',
    accent: '#fbbf24',
    icon: 'D',
  },
  night: {
    bg: 'bg-[#0d1117]',
    text: 'text-[#b8bfc7]',
    secondary: 'text-[#6e7681]',
    border: 'phantasi-reader__line',
    surface: 'phantasi-reader__chip',
    surfaceSolid: 'phantasi-reader__chip is-solid',
    accent: '#58a6ff',
    icon: 'N',
  },
}

export const THEME_ORDER: ThemeKey[] = ['light', 'sepia', 'dark', 'night']

export const LAYOUT_OPTIONS: LayoutOption[] = [
  { id: 'narrow', width: 'max-w-3xl' },
  { id: 'wide', width: 'max-w-4xl' },
]
