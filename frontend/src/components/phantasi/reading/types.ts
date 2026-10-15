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

type FontLabelKey = 'fontSerif' | 'fontSans' | 'fontSystem'

type LayoutLabelKey = 'layoutNarrow' | 'layoutWide'

export interface FontOption {
  id: string
  labelKey: FontLabelKey
  family: string
}

export interface LayoutOption {
  id: LayoutKey
  labelKey: LayoutLabelKey
  width: string
}
