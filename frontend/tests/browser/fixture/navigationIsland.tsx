import { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter, useNavigate } from 'react-router-dom'
import NavigationIsland from '../../../src/components/NavigationIsland'
import { AuthProvider } from '../../../src/contexts/AuthContext'
import { I18nProvider } from '../../../src/contexts/I18nContext'
import { NavigationProvider, useSecondaryNav } from '../../../src/contexts/NavigationContext'
import '../../../src/styles/navigation-island.css'

const items = [{ id: 'all', label: 'All', icon: 'All' }]
function Harness() {
  const navigate = useNavigate()
  const { setExpanded } = useSecondaryNav({ routePath: '/library', items, defaultActiveId: 'all' })
  useEffect(() => {
    const onExpand = (event: Event) => {
      const path = (event as CustomEvent<{ path: string }>).detail?.path
      if (path === '/library' || path?.startsWith('/library/')) setExpanded(true)
    }
    window.addEventListener('nav-expand-secondary', onExpand)
    return () => window.removeEventListener('nav-expand-secondary', onExpand)
  }, [setExpanded])
  return <>
    <button onClick={() => navigate('/library/canvas')}>Open shelf</button>
    <NavigationIsland />
  </>
}
export function mountNavigationIsland() {
  createRoot(document.getElementById('root')!).render(
    <BrowserRouter><I18nProvider><AuthProvider><NavigationProvider><Harness /></NavigationProvider></AuthProvider></I18nProvider></BrowserRouter>,
  )
}
