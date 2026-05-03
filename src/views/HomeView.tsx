import { useState, useEffect } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { Plus, Folder, MoreVertical, FolderInput, Trash2 } from 'lucide-react'
import BrandLogo from '../components/BrandLogo'
import { MOCK_CHATS, type MockChat } from '../data/mockChats'
import { MOCK_FOLDERS, type MockFolder } from '../data/mockFolders'

const C_TEXT  = '#C8E8F4'
const C_MUTED = 'rgba(58, 138, 170, 0.48)'
const C_BASE  = '#3A8AAA'
const C_PEAK  = '#86D5F2'

function relativeTime(date: Date): string {
  const ms = Date.now() - date.getTime()
  const m  = Math.floor(ms / 60_000)
  const hh = Math.floor(ms / 3_600_000)
  const d  = Math.floor(ms / 86_400_000)
  if (m  < 60)  return `${m}m ago`
  if (hh < 24)  return `${hh}h ago`
  if (d  === 1) return 'yesterday'
  if (d  <  7)  return `${d} days ago`
  return date.toLocaleDateString()
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div style={{
      fontSize: 10.5,
      color: 'rgba(58,138,170,0.38)',
      letterSpacing: '0.10em',
      textTransform: 'uppercase' as const,
      userSelect: 'none' as const,
    }}>
      {children}
    </div>
  )
}

interface MenuRowProps {
  icon:     React.ReactNode
  label:    string
  hasArrow?: boolean
  danger?:  boolean
  onClick:  (e: React.MouseEvent) => void
}

function MenuRow({ icon, label, hasArrow, danger, onClick }: MenuRowProps) {
  const [hov, setHov] = useState(false)
  const col = danger
    ? (hov ? '#c47070' : '#a85a5a')
    : (hov ? C_TEXT    : C_MUTED)

  return (
    <div
      onMouseEnter={() => setHov(true)}
      onMouseLeave={() => setHov(false)}
      onClick={onClick}
      style={{
        display: 'flex', alignItems: 'center', gap: 10,
        padding: '8px 14px',
        cursor: 'pointer',
        background: hov
          ? (danger ? 'rgba(168,90,90,0.09)' : 'rgba(58,138,170,0.09)')
          : 'transparent',
        transition: 'background 0.12s',
        userSelect: 'none' as const,
      }}
    >
      <span style={{ color: col, display: 'flex', alignItems: 'center', flexShrink: 0 }}>
        {icon}
      </span>
      <span style={{ fontSize: 13, color: col, flex: 1 }}>{label}</span>
      {hasArrow && (
        <span style={{ color: col, fontSize: 12, opacity: 0.7, lineHeight: 1 }}>›</span>
      )}
    </div>
  )
}

interface FolderCardProps {
  folder:      MockFolder
  isDropOver:  boolean
  onDragOver:  (e: React.DragEvent) => void
  onDragLeave: () => void
  onDrop:      (e: React.DragEvent) => void
  onClick:     () => void
}

function FolderCard({ folder, isDropOver, onDragOver, onDragLeave, onDrop, onClick }: FolderCardProps) {
  const [hov, setHov] = useState(false)
  const active = hov || isDropOver

  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHov(true)}
      onMouseLeave={() => setHov(false)}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
      style={{
        background: active ? 'rgba(58,138,170,0.12)' : 'rgba(17,22,31,0.55)',
        backdropFilter: 'blur(16px)',
        WebkitBackdropFilter: 'blur(16px)',
        border: `1px solid ${isDropOver ? 'rgba(134,213,242,0.45)' : active ? 'rgba(58,138,170,0.35)' : 'rgba(58,138,170,0.11)'}`,
        borderRadius: 11,
        padding: '12px 16px',
        cursor: 'pointer',
        transition: 'background 0.16s, border-color 0.16s, box-shadow 0.16s',
        boxShadow: isDropOver
          ? '0 0 22px rgba(58,138,170,0.30), 0 0 6px rgba(134,213,242,0.12)'
          : hov ? '0 0 14px rgba(58,138,170,0.10)' : 'none',
        display: 'flex',
        flexDirection: 'column' as const,
        gap: 4,
        minWidth: 108,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Folder
          size={17}
          strokeWidth={1.8}
          color={isDropOver ? C_PEAK : active ? C_TEXT : C_BASE}
          style={{ transition: 'color 0.16s', flexShrink: 0 }}
        />
        <span style={{
          fontSize: 13.5,
          fontWeight: 500,
          color: active ? C_TEXT : C_TEXT,
          whiteSpace: 'nowrap' as const,
        }}>
          {folder.name}
        </span>
      </div>
      <div style={{ fontSize: 11, color: C_MUTED, paddingLeft: 25 }}>
        {folder.chatCount} chats
      </div>
    </div>
  )
}

interface ChatCardProps {
  chat:        MockChat
  index:       number
  isDragging:  boolean
  onNavigate:  () => void
  onMenuOpen:  (e: React.MouseEvent) => void
  onDragStart: (e: React.DragEvent) => void
  onDragEnd:   () => void
}

function ChatCard({ chat, index, isDragging, onNavigate, onMenuOpen, onDragStart, onDragEnd }: ChatCardProps) {
  const [hov, setHov] = useState(false)

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: isDragging ? 0.65 : 1, y: 0 }}
      transition={{ duration: 0.28, delay: isDragging ? 0 : 0.38 + index * 0.06 }}
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={onNavigate}
      onMouseEnter={() => setHov(true)}
      onMouseLeave={() => setHov(false)}
      style={{
        background: hov ? 'rgba(58,138,170,0.10)' : 'rgba(17,22,31,0.55)',
        backdropFilter: 'blur(16px)',
        WebkitBackdropFilter: 'blur(16px)',
        border: `1px solid ${hov ? 'rgba(58,138,170,0.30)' : 'rgba(58,138,170,0.11)'}`,
        borderRadius: 13,
        padding: '14px 16px',
        cursor: isDragging ? 'grabbing' : 'grab',
        transition: 'background 0.16s, border-color 0.16s, box-shadow 0.16s, transform 0.12s',
        boxShadow: isDragging
          ? '0 14px 40px rgba(0,0,0,0.55), 0 0 20px rgba(58,138,170,0.15)'
          : hov ? '0 0 22px rgba(58,138,170,0.11)' : '0 2px 10px rgba(0,0,0,0.25)',
        transform: isDragging ? 'scale(1.03)' : 'scale(1)',
        display: 'flex',
        flexDirection: 'column' as const,
        gap: 6,
        minHeight: 90,
        position: 'relative' as const,
        userSelect: 'none' as const,
      }}
    >
      {/* 3-dot menu button — visible on hover */}
      <div
        style={{
          position: 'absolute' as const,
          top: 9, right: 9,
          opacity: hov ? 1 : 0,
          transition: 'opacity 0.14s',
          zIndex: 2,
        }}
        onClick={onMenuOpen}
      >
        <div
          style={{
            padding: '3px 4px',
            borderRadius: 5,
            color: C_MUTED,
            display: 'flex',
            alignItems: 'center',
            cursor: 'pointer',
          }}
          onMouseEnter={e => {
            ;(e.currentTarget as HTMLDivElement).style.background = 'rgba(58,138,170,0.18)'
            ;(e.currentTarget as HTMLDivElement).style.color = C_TEXT
          }}
          onMouseLeave={e => {
            ;(e.currentTarget as HTMLDivElement).style.background = 'transparent'
            ;(e.currentTarget as HTMLDivElement).style.color = C_MUTED
          }}
        >
          <MoreVertical size={14} strokeWidth={2} />
        </div>
      </div>

      <div style={{
        fontSize: 13.5,
        fontWeight: 500,
        color: C_TEXT,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap' as const,
        paddingRight: 22,
      }}>
        {chat.title}
      </div>
      <div style={{
        fontSize: 12,
        color: C_MUTED,
        display: '-webkit-box',
        WebkitLineClamp: 2,
        WebkitBoxOrient: 'vertical' as const,
        overflow: 'hidden',
        lineHeight: 1.5,
        flex: 1,
      }}>
        {chat.lastMessage}
      </div>
      <div style={{ fontSize: 10.5, color: 'rgba(58,138,170,0.32)', marginTop: 2 }}>
        {relativeTime(chat.updatedAt)}
      </div>
    </motion.div>
  )
}

interface MenuState {
  chatId: string
  x:      number
  y:      number
}

interface Props {
  onNewChat:    () => void
  onSelectChat: (chatId: string) => void
}

export default function HomeView({ onNewChat, onSelectChat }: Props) {
  const [btnHov,      setBtnHov]      = useState(false)
  const [menu,        setMenu]        = useState<MenuState | null>(null)
  const [showSub,     setShowSub]     = useState(false)
  const [draggingId,  setDraggingId]  = useState<string | null>(null)
  const [overFolder,  setOverFolder]  = useState<string | null>(null)

  // Close menu when clicking anywhere outside it
  useEffect(() => {
    if (!menu) return
    const handler = () => { setMenu(null); setShowSub(false) }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [menu])

  const openMenu = (e: React.MouseEvent, chatId: string) => {
    e.stopPropagation()
    e.preventDefault()
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    setMenu({ chatId, x: rect.right - 188, y: rect.bottom + 6 })
    setShowSub(false)
  }

  const handleFolderDrop = (e: React.DragEvent, folderId: string) => {
    e.preventDefault()
    const chatId = e.dataTransfer.getData('chatId')
    if (chatId) console.log(`moved chat ${chatId} to folder ${folderId}`)
    setOverFolder(null)
    setDraggingId(null)
  }

  return (
    <div style={{
      width: '100%', height: '100%',
      display: 'flex', overflow: 'hidden',
      position: 'relative',
    }}>

      {/* ── Left column 50% ── folders + recent chats ── */}
      <div style={{
        flex: '0 0 50%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        padding: '36px 28px 36px 36px',
        borderRight: '1px solid rgba(58,138,170,0.07)',
      }}>

        {/* Folders section */}
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.35, delay: 0.12 }}
          style={{ flexShrink: 0 }}
        >
          <SectionLabel>Folders</SectionLabel>
          <div style={{ height: 14 }} />
          {MOCK_FOLDERS.length === 0 ? (
            <div style={{ fontSize: 13, color: C_MUTED, fontStyle: 'italic' }}>No folders.</div>
          ) : (
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10 }}>
              {MOCK_FOLDERS.map(folder => (
                <FolderCard
                  key={folder.id}
                  folder={folder}
                  isDropOver={overFolder === folder.id}
                  onDragOver={e => { e.preventDefault(); setOverFolder(folder.id) }}
                  onDragLeave={() => setOverFolder(null)}
                  onDrop={e => handleFolderDrop(e, folder.id)}
                  onClick={() => console.log(`open folder ${folder.id}`)}
                />
              ))}
            </div>
          )}
        </motion.div>

        {/* Divider */}
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3, delay: 0.22 }}
          style={{ height: 1, background: 'rgba(58,138,170,0.12)', margin: '22px 0', flexShrink: 0 }}
        />

        {/* Recent header */}
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.35, delay: 0.30 }}
          style={{ flexShrink: 0, marginBottom: 14 }}
        >
          <SectionLabel>Recent</SectionLabel>
        </motion.div>

        {/* Scrollable cards grid — ONLY scrollable area */}
        <div style={{
          flex: 1,
          minHeight: 0,
          overflowY: 'auto',
          overflowX: 'hidden',
          overscrollBehavior: 'contain',
          scrollbarWidth: 'thin' as const,
          paddingRight: 4,
        }}>
          {MOCK_CHATS.length === 0 ? (
            <div style={{
              height: '100%',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              color: C_MUTED, fontSize: 13, fontStyle: 'italic',
            }}>
              No conversations yet.
            </div>
          ) : (
            <div style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(2, 1fr)',
              gap: 11,
              paddingBottom: 20,
            }}>
              {MOCK_CHATS.map((chat, idx) => (
                <ChatCard
                  key={chat.id}
                  chat={chat}
                  index={idx}
                  isDragging={draggingId === chat.id}
                  onNavigate={() => onSelectChat(chat.id)}
                  onMenuOpen={e => openMenu(e, chat.id)}
                  onDragStart={e => {
                    setDraggingId(chat.id)
                    e.dataTransfer.setData('chatId', chat.id)
                    e.dataTransfer.effectAllowed = 'move'
                  }}
                  onDragEnd={() => { setDraggingId(null); setOverFolder(null) }}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      {/* ── Right column 50% ── brand block ── */}
      <div style={{
        flex: '0 0 50%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '44px 40px',
        overflow: 'hidden',
      }}>

        <motion.div
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.45 }}
        >
          <BrandLogo style={{ width: 180, height: 180 }} />
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.45, delay: 0.08 }}
          style={{
            fontSize: 52,
            fontWeight: 250,
            color: C_TEXT,
            letterSpacing: '0.04em',
            marginTop: 16,
            lineHeight: 1,
            userSelect: 'none',
          }}
        >
          ARIA
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.45, delay: 0.13 }}
          style={{
            fontSize: 17,
            fontWeight: 500,
            color: C_TEXT,
            letterSpacing: '0.09em',
            marginTop: 7,
            userSelect: 'none',
            textAlign: 'center',
          }}
        >
          Advanced Researching &amp; Intelligence Assistant
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.45, delay: 0.19 }}
          style={{
            fontSize: 13,
            color: C_MUTED,
            fontStyle: 'italic',
            marginTop: 13,
            letterSpacing: '0.01em',
            userSelect: 'none',
          }}
        >
          A presence, not a service.
        </motion.div>

        <motion.button
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.38, delay: 0.26 }}
          onClick={onNewChat}
          onMouseEnter={() => setBtnHov(true)}
          onMouseLeave={() => setBtnHov(false)}
          style={{
            marginTop: 36,
            display: 'flex', alignItems: 'center', gap: 8,
            padding: '10px 26px',
            borderRadius: 32,
            background: btnHov ? 'rgba(58,138,170,0.14)' : 'rgba(58,138,170,0.07)',
            border: `1px solid ${btnHov ? 'rgba(58,138,170,0.42)' : 'rgba(58,138,170,0.20)'}`,
            color: btnHov ? C_PEAK : C_BASE,
            fontSize: 13.5,
            fontFamily: 'inherit',
            cursor: 'pointer',
            letterSpacing: '0.02em',
            boxShadow: btnHov ? '0 0 18px rgba(58,138,170,0.16)' : 'none',
            transition: 'background 0.16s, border-color 0.16s, color 0.16s, box-shadow 0.16s',
          }}
        >
          <Plus size={14} strokeWidth={2.2} />
          New conversation
        </motion.button>

      </div>

      {/* ── Context menu — fixed, rendered above everything ── */}
      <AnimatePresence>
        {menu && (
          <motion.div
            key="ctx-menu"
            initial={{ opacity: 0, scale: 0.95, y: -4 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: -4 }}
            transition={{ duration: 0.10, ease: 'easeOut' }}
            onMouseDown={e => e.stopPropagation()}
            style={{
              position: 'fixed',
              top: menu.y,
              left: menu.x,
              zIndex: 1000,
              background: 'rgba(11,17,26,0.94)',
              backdropFilter: 'blur(24px)',
              WebkitBackdropFilter: 'blur(24px)',
              border: '1px solid rgba(58,138,170,0.22)',
              borderRadius: 10,
              padding: '5px 0',
              minWidth: 186,
              boxShadow: '0 10px 36px rgba(0,0,0,0.55), 0 2px 8px rgba(0,0,0,0.3)',
              transformOrigin: 'top right',
            }}
          >
            {/* Move to folder row */}
            <div style={{ position: 'relative' }}>
              <MenuRow
                icon={<FolderInput size={14} strokeWidth={1.8} />}
                label="Move to folder..."
                hasArrow
                onClick={e => { e.stopPropagation(); setShowSub(s => !s) }}
              />

              <AnimatePresence>
                {showSub && (
                  <motion.div
                    key="submenu"
                    initial={{ opacity: 0, x: -6 }}
                    animate={{ opacity: 1, x: 0 }}
                    exit={{ opacity: 0, x: -6 }}
                    transition={{ duration: 0.10, ease: 'easeOut' }}
                    onMouseDown={e => e.stopPropagation()}
                    style={{
                      position: 'absolute',
                      left: '100%',
                      top: 0,
                      marginLeft: 4,
                      background: 'rgba(11,17,26,0.94)',
                      backdropFilter: 'blur(24px)',
                      WebkitBackdropFilter: 'blur(24px)',
                      border: '1px solid rgba(58,138,170,0.22)',
                      borderRadius: 10,
                      padding: '5px 0',
                      minWidth: 148,
                      boxShadow: '0 10px 36px rgba(0,0,0,0.55)',
                      zIndex: 1001,
                      transformOrigin: 'top left',
                    }}
                  >
                    {MOCK_FOLDERS.map(f => (
                      <MenuRow
                        key={f.id}
                        icon={<Folder size={13} strokeWidth={1.8} />}
                        label={f.name}
                        onClick={e => {
                          e.stopPropagation()
                          console.log(`move chat ${menu.chatId} to folder ${f.id}`)
                          setMenu(null)
                          setShowSub(false)
                        }}
                      />
                    ))}
                  </motion.div>
                )}
              </AnimatePresence>
            </div>

            {/* Divider */}
            <div style={{ height: 1, background: 'rgba(58,138,170,0.12)', margin: '4px 0' }} />

            {/* Delete chat */}
            <MenuRow
              icon={<Trash2 size={14} strokeWidth={1.8} />}
              label="Delete chat"
              danger
              onClick={e => {
                e.stopPropagation()
                console.log(`delete chat ${menu.chatId}`)
                setMenu(null)
              }}
            />
          </motion.div>
        )}
      </AnimatePresence>

    </div>
  )
}
