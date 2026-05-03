import { useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Minus, Square, X } from 'lucide-react'

const appWindow = getCurrentWindow()

function WinBtn({
  icon, onClick, danger,
}: {
  icon: React.ReactNode
  onClick: () => void
  danger?: boolean
}) {
  const [hovered, setHovered] = useState(false)
  return (
    <button
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        width: 28, height: 22, borderRadius: 5, border: 'none',
        background: hovered
          ? (danger ? 'rgba(200,70,70,0.22)' : 'rgba(58,138,170,0.14)')
          : 'transparent',
        color: hovered
          ? (danger ? 'rgba(230,110,110,0.90)' : 'rgba(134,213,242,0.85)')
          : 'rgba(58,138,170,0.28)',
        cursor: 'pointer',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        transition: 'background 0.12s, color 0.12s',
        flexShrink: 0,
      }}
    >
      {icon}
    </button>
  )
}

export default function TitleBar() {
  return (
    <div
      data-tauri-drag-region
      style={{
        height: 32, flexShrink: 0,
        background: 'rgba(10, 14, 20, 0.92)',
        backdropFilter: 'blur(12px)',
        WebkitBackdropFilter: 'blur(12px)',
        borderBottom: '1px solid rgba(58, 138, 170, 0.07)',
        display: 'flex', alignItems: 'center',
        justifyContent: 'space-between',
        padding: '0 8px 0 16px',
        position: 'relative', zIndex: 200,
        cursor: 'default',
      }}
    >
      <span style={{
        color: 'rgba(58, 138, 170, 0.32)',
        fontSize: 11, letterSpacing: '0.20em',
        userSelect: 'none', pointerEvents: 'none',
      }}>
        aria
      </span>

      <div style={{ display: 'flex', gap: 2 }}>
        <WinBtn
          icon={<Minus size={10} strokeWidth={2} />}
          onClick={() => appWindow.minimize()}
        />
        <WinBtn
          icon={<Square size={9} strokeWidth={2} />}
          onClick={() => appWindow.toggleMaximize()}
        />
        <WinBtn
          icon={<X size={11} strokeWidth={2} />}
          onClick={() => appWindow.close()}
          danger
        />
      </div>
    </div>
  )
}
