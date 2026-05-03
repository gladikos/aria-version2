import { useEffect } from 'react'
import { motion } from 'framer-motion'
import AriaLogo from './components/AriaLogo'
import ChatPanel from './components/ChatPanel'
import TitleBar from './components/TitleBar'
import { useAriaState } from './hooks/useAriaState'

const BLOBS = [
  { left: '12%', top: '25%', size: 560, dur: 48, delay: 0,  x: [0, 28, -18, 12, 0] as number[],  y: [0, -18, 22, -8, 0] as number[]  },
  { left: '82%', top: '62%', size: 480, dur: 58, delay: 14, x: [0, -22, 16, -30, 0] as number[], y: [0, 25, -14, 18, 0] as number[]  },
  { left: '52%', top: '88%', size: 400, dur: 40, delay: 7,  x: [0, 15, -25, 20, 0] as number[],  y: [0, -30, 10, -20, 0] as number[] },
]

export default function App() {
  const { state, setState } = useAriaState()

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return
      if (e.key === '1') setState('idle')
      else if (e.key === '2') setState('thinking')
      else if (e.key === '3') setState('speaking')
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [setState])

  return (
    <div style={{
      display: 'flex', flexDirection: 'column',
      height: '100%', overflow: 'hidden',
      background: '#0A0E14', position: 'relative',
    }}>

      {/* Dot grid */}
      <div style={{
        position: 'absolute', inset: 0,
        backgroundImage: 'radial-gradient(circle, rgba(58,138,170,0.09) 1px, transparent 1px)',
        backgroundSize: '40px 40px',
        pointerEvents: 'none', zIndex: 0,
      }} />

      {/* Ambient glow blobs */}
      {BLOBS.map((b, i) => (
        <motion.div
          key={i}
          style={{
            position: 'absolute',
            width: b.size, height: b.size,
            left: b.left, top: b.top,
            transform: 'translate(-50%, -50%)',
            borderRadius: '50%',
            background: 'radial-gradient(circle, rgba(58,138,170,0.065) 0%, rgba(134,213,242,0.028) 45%, transparent 70%)',
            filter: 'blur(50px)',
            pointerEvents: 'none', zIndex: 0,
          }}
          animate={{ x: b.x, y: b.y }}
          transition={{ duration: b.dur, repeat: Infinity, ease: 'easeInOut', delay: b.delay }}
        />
      ))}

      {/* Vignette */}
      <div style={{
        position: 'absolute', inset: 0,
        background: 'radial-gradient(ellipse at 50% 48%, transparent 38%, rgba(0,0,0,0.52) 100%)',
        pointerEvents: 'none', zIndex: 1,
      }} />

      {/* Title bar */}
      <TitleBar />

      {/* Main content */}
      <div style={{
        flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden',
        position: 'relative', zIndex: 2,
      }}>
        <div className="logo-column">
          <AriaLogo state={state} style={{ width: 'min(400px, 78%)', height: 'auto' }} />
        </div>
        <div className="chat-column">
          <ChatPanel onStateChange={setState} />
        </div>
      </div>

      {/* Debug state — barely visible */}
      <div style={{
        position: 'fixed', bottom: 10, left: 12,
        fontSize: 10, fontFamily: 'monospace',
        color: '#3A8AAA', opacity: 0.07,
        userSelect: 'none', letterSpacing: '0.08em',
        pointerEvents: 'none', zIndex: 10,
      }}>
        {state}
      </div>

    </div>
  )
}
