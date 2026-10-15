export type ThemeKey = 'light' | 'sepia' | 'dark' | 'night'

export type LayoutKey = 'narrow' | 'wide'

export interface ThemeConfig {
  bg: string
  text: string
  secondary: string
  border: string
  surface: string
  surfaceSolid: string
  accent: string
  icon: string
}

export interface FontOption {
  id: string
  family: string
}

export interface LayoutOption {
  id: LayoutKey
  width: string
}
