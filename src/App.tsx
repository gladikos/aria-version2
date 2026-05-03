import { useState, useEffect } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { ArrowLeft } from 'lucide-react'
import AriaLogo from './components/AriaLogo'
import ChatPanel from './components/ChatPanel'
import TitleBar from './components/TitleBar'
import HomeView from './views/HomeView'
import { useAriaState } from './hooks/useAriaState'
import { initDb, createChat, listMessages, type Chat } from './lib/db'
import type { ChatMessage } from './hooks/useChat'

const C_TEXT  = '#C8E8F4'
const C_MUTED = 'rgba(58, 138, 170, 0.48)'

const BLOBS = [
  { left: '12%', top: '25%', size: 560, dur: 48, delay: 0,  x: [0, 28, -18, 12, 0] as number[],  y: [0, -18, 22, -8, 0] as number[]  },
  { left: '82%', top: '62%', size: 480, dur: 58, delay: 14, x: [0, -22, 16, -30, 0] as number[], y: [0, 25, -14, 18, 0] as number[]  },
  { left: '52%', top: '88%', size: 400, dur: 40, delay: 7,  x: [0, 15, -25, 20, 0] as number[],  y: [0, -30, 10, -20, 0] as number[] },
]

export default function App() {
  const { state, setState } = useAriaState()

  const [dbReady,    setDbReady]    = useState(false)
  const [view,       setView]       = useState<'home' | 'chat'>('home')
  const [activeChat, setActiveChat] = useState<Chat | null>(null)
  const [chatMsgs,   setChatMsgs]   = useState<ChatMessage[]>([])
  const [chatKey,    setChatKey]    = useState(0)
  const [homeKey,    setHomeKey]    = useState(0)
  const [backHov,    setBackHov]    = useState(false)

  // Initialise DB on mount — all table creation and seeding happens here
  useEffect(() => {
    initDb()
      .then(() => setDbReady(true))
      .catch(err => {
        console.error('[aria] DB init error:', err)
        setDbReady(true) // fail open — allow use without persistence
      })
  }, [])

  const goToNewChat = async () => {
    try {
      const chat = await createChat(null)
      setActiveChat(chat)
      setChatMsgs([])
      setChatKey(k => k + 1)
      setView('chat')
    } catch (err) {
      console.error('[aria] failed to create chat:', err)
    }
  }

  const goToChat = async (chatId: string, chatMeta: Chat) => {
    try {
      const msgs = await listMessages(chatId)
      const chatMessages: ChatMessage[] = msgs.map(m => ({
        id:   m.id,
        role: m.role === 'assistant' ? 'aria' : 'user',
        content: m.content,
      }))
      setActiveChat(chatMeta)
      setChatMsgs(chatMessages)
      setChatKey(k => k + 1)
      setView('chat')
    } catch (err) {
      console.error('[aria] failed to load messages:', err)
    }
  }

  const goHome = () => {
    setView('home')
    setHomeKey(k => k + 1) // remounts HomeView → fresh data load
  }

  // Debug key shortcuts — only active in chat view
  useEffect(() => {
    if (view !== 'chat') return
    const handler = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return
      if (e.key === '1') setState('idle')
      else if (e.key === '2') setState('thinking')
      else if (e.key === '3') setState('speaking')
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [setState, view])

  // Don't render until DB is ready
  if (!dbReady) return null

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

      {/* View router */}
      <AnimatePresence mode="wait" initial={false}>
        {view === 'home' ? (

          <motion.div
            key={`home-${homeKey}`}
            style={{
              flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden',
              position: 'relative', zIndex: 2,
            }}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.26 }}
          >
            <HomeView onNewChat={goToNewChat} onSelectChat={goToChat} />
          </motion.div>

        ) : (

          <motion.div
            key="chat"
            style={{
              flex: 1, display: 'flex', minHeight: 0, overflow: 'hidden',
              position: 'relative', zIndex: 2,
            }}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.26 }}
          >
            {/* Back button + chat title */}
            <div style={{
              position: 'absolute', top: 14, left: 18, zIndex: 10,
              display: 'flex', alignItems: 'center', gap: 9,
            }}>
              <button
                onClick={goHome}
                onMouseEnter={() => setBackHov(true)}
                onMouseLeave={() => setBackHov(false)}
                style={{
                  background: 'transparent', border: 'none',
                  padding: '5px', cursor: 'pointer',
                  color: backHov ? C_TEXT : C_MUTED,
                  display: 'flex', alignItems: 'center',
                  transform: backHov ? 'scale(1.12)' : 'scale(1)',
                  transition: 'color 0.14s, transform 0.14s',
                  borderRadius: 6,
                }}
              >
                <ArrowLeft size={17} strokeWidth={2} />
              </button>

              <span style={{
                fontSize: 11,
                color: activeChat?.title
                  ? 'rgba(58,138,170,0.42)'
                  : 'rgba(58,138,170,0.28)',
                fontStyle: activeChat?.title ? 'normal' : 'italic',
                letterSpacing: '0.02em',
                userSelect: 'none',
              }}>
                {activeChat?.title ?? 'New conversation'}
              </span>
            </div>

            <div className="logo-column">
              <AriaLogo state={state} style={{ width: 'min(400px, 78%)', height: 'auto' }} />
            </div>
            <div className="chat-column">
              <ChatPanel
                key={chatKey}
                chatId={activeChat!.id}
                initialMessages={chatMsgs}
                onStateChange={setState}
                onTitleGenerated={title =>
                  setActiveChat(prev => prev ? { ...prev, title } : prev)
                }
              />
            </div>
          </motion.div>

        )}
      </AnimatePresence>

      {/* Debug state */}
      <div style={{
        position: 'fixed', bottom: 10, left: 12,
        fontSize: 10, fontFamily: 'monospace',
        color: '#3A8AAA', opacity: 0.07,
        userSelect: 'none', letterSpacing: '0.08em',
        pointerEvents: 'none', zIndex: 10,
      }}>
        {view === 'chat' ? state : ''}
      </div>
    </div>
  )
}
