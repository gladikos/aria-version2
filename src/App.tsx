import AriaLogo from './components/AriaLogo'
import { useAriaState } from './hooks/useAriaState'
import { useEffect } from 'react'

export default function App() {
  const { state, setState } = useAriaState()

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === '1') setState('idle')
      else if (e.key === '2') setState('thinking')
      else if (e.key === '3') setState('speaking')
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [setState])

  return (
    <div
      className="flex items-center justify-center h-full w-full"
      style={{ backgroundColor: '#0A0E14', color: '#3A8AAA' }}
    >
      <AriaLogo state={state} width={400} height={400} />
      <div style={{
        position: 'fixed', bottom: 12, left: 14,
        fontSize: 11, fontFamily: 'monospace',
        color: '#3A8AAA', opacity: 0.4,
        userSelect: 'none', letterSpacing: '0.08em',
        pointerEvents: 'none',
      }}>
        state: {state}
      </div>
    </div>
  )
}
