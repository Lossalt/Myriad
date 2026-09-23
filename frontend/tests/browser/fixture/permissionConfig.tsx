import React from 'react'
import { createRoot } from 'react-dom/client'
import { updatePermissionsConfig } from '../../../src/services/configApi'
import { canUseAgent, DEFAULT_MODULE_VISIBILITY_PREFERENCES } from '../../../src/utils/moduleVisibility'
import { usePermissionConfig } from '../../../src/utils/permissionConfig'

function Fixture() {
  const { loaded, elevatedAiChat } = usePermissionConfig()
  const allowed = canUseAgent(DEFAULT_MODULE_VISIBILITY_PREFERENCES, { isAuthenticated: true, isAdmin: false }, elevatedAiChat)
  return <>
    <output data-permission>{loaded ? String(allowed) : 'loading'}</output>
    <button onClick={() => { void updatePermissionsConfig({ user_perm_ai_chat: true }) }}>Save chat permission</button>
  </>
}
export function mount() { createRoot(document.getElementById('root')!).render(<Fixture />) }
