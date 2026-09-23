import { useLayoutEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { useTitleFont } from '../../../src/hooks/useTitleFont'
import { useWidgetTheme } from '../../../src/hooks/useWidgetTheme'
import { authSubject } from '../../../src/utils/authSubject'

function Reader({ id }: { id: string }) {
  const { titleFontSize } = useTitleFont()
  return <output data-reader={id}>{titleFontSize}</output>
}
function Editor() {
  const { setSurface, setGlowMode } = useWidgetTheme()
  const { setTitleFontSize, setTitleFont, setTitleColor, titleFont, titleColor, isLoading } = useTitleFont()
  // Publish between a sibling's initial render and passive subscription.
  useLayoutEffect(() => { setTitleFontSize(1.2) }, [setTitleFontSize])
  return <><button onClick={() => { setSurface('outline'); setGlowMode('none') }}>Save widget theme</button><button onClick={() => authSubject.change('next', true)}>Change subject</button><button onClick={() => setTitleFontSize(1.4)}>Save large</button><button onClick={() => { setTitleFontSize(0.8); setTitleColor('accent') }}>Save size and color</button><button onClick={() => setTitleFontSize(1.4)}>Change size</button><button onClick={() => void setTitleFont('codystar')}>Font A</button><button onClick={() => void setTitleFont('henny-penny')}>Font B</button><span data-font>{titleFont}</span><span data-color>{titleColor}</span><span data-loading>{String(isLoading)}</span></>
}
function Fixture() {
  const [late, setLate] = useState(false)
  return <><Reader id="early" /><Editor /><button onClick={() => setLate(value => !value)}>Toggle reader</button>{late && <Reader id="late" />}</>
}
createRoot(document.getElementById('root')!).render(<Fixture />)
