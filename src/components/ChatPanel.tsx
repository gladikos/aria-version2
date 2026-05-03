import { useRef, useEffect, useState, useCallback, createContext, useContext } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Components } from 'react-markdown'
import {
  ArrowUp, Search, Globe, Folder, FileText, Pencil,
  FolderPlus, ArrowRight, Copy, Trash2, ExternalLink,
  Terminal, Info, AlertCircle, Check, MoveRight,
} from 'lucide-react'
import type { AriaState } from '../hooks/useAriaState'
import { useChat } from '../hooks/useChat'

// ─── Colors ───────────────────────────────────────────────────────────────────
const C_BASE  = '#3A8AAA'
const C_PEAK  = '#86D5F2'
const C_TEXT  = '#C8E8F4'
const C_MUTED = 'rgba(58, 138, 170, 0.48)'

const C_AMBER       = '#d4a574'
const C_AMBER_DIM   = 'rgba(212, 165, 116, 0.45)'
const C_AMBER_GHOST = 'rgba(212, 165, 116, 0.10)'
const C_AMBER_TEXT  = 'rgba(240, 210, 170, 0.88)'

// ─── Tool metadata ────────────────────────────────────────────────────────────
const TOOL_META: Record<string, { icon: React.ReactNode; label: string }> = {
  web_search:           { icon: <Search size={10} />,       label: 'searching the web'    },
  fetch_url:            { icon: <Globe size={10} />,        label: 'fetching page'         },
  list_directory:       { icon: <Folder size={10} />,       label: 'reading directory'     },
  read_file:            { icon: <FileText size={10} />,     label: 'reading file'          },
  write_file:           { icon: <Pencil size={10} />,       label: 'writing file'          },
  search_filesystem:    { icon: <Search size={10} />,       label: 'searching files'       },
  create_directory:     { icon: <FolderPlus size={10} />,   label: 'creating folder'       },
  move_path:            { icon: <MoveRight size={10} />,    label: 'moving'                },
  copy_path:            { icon: <Copy size={10} />,         label: 'copying'               },
  delete_path:          { icon: <Trash2 size={10} />,       label: 'deleting'              },
  open_in_app:          { icon: <ExternalLink size={10} />, label: 'opening'               },
  run_command:          { icon: <Terminal size={10} />,     label: 'running command'       },
  get_path_info:        { icon: <Info size={10} />,         label: 'checking path'         },
  request_confirmation: { icon: <AlertCircle size={10} />,  label: 'confirming action'     },
}

// ─── Greeting ─────────────────────────────────────────────────────────────────
function greeting(): string {
  const h = new Date().getHours()
  if (h < 5)  return 'Still up?'
  if (h < 12) return 'Morning. What can I do?'
  if (h < 17) return "Hey. What's up?"
  if (h < 21) return 'Evening. What do you need?'
  return 'Hey. What can I do?'
}

// ─── Context for inline-vs-block code detection ───────────────────────────────
const InsidePreCtx = createContext(false)

// ─── Markdown components ─────────────────────────────────────────────────────
function makeMarkdownComponents(streaming?: boolean): Components {
  return {
    p: ({ children }) => (
      <p style={{ margin: '0 0 6px 0' }}>{children}</p>
    ),
    strong: ({ children }) => (
      <strong style={{ color: C_TEXT, fontWeight: 600 }}>{children}</strong>
    ),
    em: ({ children }) => (
      <em style={{ color: C_TEXT }}>{children}</em>
    ),
    pre: ({ children }) => (
      <InsidePreCtx.Provider value={true}>
        <div style={{
          background: 'rgba(10,14,20,0.65)',
          border: '1px solid rgba(58,138,170,0.18)',
          borderRadius: 8, padding: '10px 14px',
          margin: '8px 0', overflowX: 'auto',
          fontSize: 12, lineHeight: 1.6,
        }}>
          {children}
        </div>
      </InsidePreCtx.Provider>
    ),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    code: (({ children, className }: any) => {
      const insidePre = useContext(InsidePreCtx)
      if (insidePre) {
        return (
          <code style={{ fontFamily: 'monospace', color: C_PEAK, display: 'block' }}
            className={className}>
            {children}
          </code>
        )
      }
      return (
        <code style={{
          background: 'rgba(58,138,170,0.14)',
          border: '1px solid rgba(58,138,170,0.18)',
          borderRadius: 4, padding: '1px 6px',
          fontSize: '0.88em', fontFamily: 'monospace',
          color: C_PEAK,
        }}>
          {children}
        </code>
      )
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    }) as any,
    a: ({ href, children }) => (
      <a
        href={href}
        onClick={e => { e.preventDefault(); if (href) window.open(href, '_blank') }}
        style={{ color: C_PEAK, textDecoration: 'underline', cursor: 'pointer' }}
      >
        {children}
      </a>
    ),
    ul: ({ children }) => <ul style={{ margin: '4px 0 6px', paddingLeft: 18 }}>{children}</ul>,
    ol: ({ children }) => <ol style={{ margin: '4px 0 6px', paddingLeft: 18 }}>{children}</ol>,
    li: ({ children }) => <li style={{ color: C_TEXT }}>{children}</li>,
    // append streaming cursor after the last rendered paragraph
    ...(streaming ? {} : {}),
  }
}

// ─── Aria message bubble ──────────────────────────────────────────────────────
function AriaBubble({ content, streaming, error }: {
  content: string; streaming?: boolean; error?: boolean
}) {
  const [hovered, setHovered] = useState(false)
  const [copied,  setCopied]  = useState(false)
  const mdComponents = makeMarkdownComponents(streaming)

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(content)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch { /* clipboard not available */ }
  }, [content])

  return (
    <div
      style={{ position: 'relative', maxWidth: '85%' }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <div style={{
        position: 'relative',
        background: error ? 'rgba(180, 70, 70, 0.12)' : 'rgba(58, 138, 170, 0.08)',
        padding: '10px 14px',
        borderRadius: '12px 12px 12px 2px',
        fontSize: 14, lineHeight: 1.6,
        color: error ? 'rgba(220, 130, 130, 0.85)' : C_TEXT,
        wordBreak: 'break-word',
      }}>
        {error ? (
          <span>{content}</span>
        ) : (
          <div className="aria-md">
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
              {content}
            </ReactMarkdown>
          </div>
        )}
        {streaming && (
          <span className="cursor-blink" style={{
            display: 'inline-block', width: 1.5, height: '1em',
            background: C_PEAK, marginLeft: 2,
            verticalAlign: 'text-bottom', borderRadius: 1,
          }} />
        )}
      </div>

      {/* Copy button */}
      <AnimatePresence>
        {hovered && !streaming && content && (
          <motion.button
            initial={{ opacity: 0, scale: 0.8 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.8 }}
            transition={{ duration: 0.1 }}
            onClick={handleCopy}
            title="Copy"
            style={{
              position: 'absolute', top: 6, right: 6,
              width: 22, height: 22, borderRadius: 5,
              border: '1px solid rgba(58,138,170,0.18)',
              background: 'rgba(10,14,20,0.80)',
              color: copied ? 'rgba(100,220,140,0.85)' : 'rgba(58,138,170,0.55)',
              cursor: 'pointer',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              transition: 'color 0.15s',
              padding: 0,
            }}
          >
            {copied ? <Check size={10} /> : <Copy size={10} />}
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  )
}

// ─── Tool indicator pill ──────────────────────────────────────────────────────
function ToolPill({ toolName }: { toolName: string }) {
  const meta = TOOL_META[toolName] ?? { icon: <Info size={10} />, label: toolName }
  return (
    <div style={{
      display: 'inline-flex', alignItems: 'center', gap: 5,
      background: 'rgba(58,138,170,0.08)',
      border: '1px solid rgba(58,138,170,0.16)',
      borderRadius: 20, padding: '4px 11px',
      fontSize: 11, color: 'rgba(134,213,242,0.68)',
      letterSpacing: '0.025em',
    }}>
      <span className="tool-dot" style={{
        display: 'inline-block', width: 5, height: 5,
        borderRadius: '50%', background: C_BASE, flexShrink: 0,
      }} />
      <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        {meta.icon}
        {meta.label}
      </span>
    </div>
  )
}

// ─── Main component ───────────────────────────────────────────────────────────
interface Props {
  onStateChange: (s: AriaState) => void
}

export default function ChatPanel({ onStateChange }: Props) {
  const { messages, input, setInput, submit, resolveConfirm, busy, currentTool } = useChat(onStateChange)
  const transcriptRef = useRef<HTMLDivElement>(null)
  const inputRef      = useRef<HTMLInputElement>(null)
  const hasText       = input.trim().length > 0

  // Scroll transcript to bottom — use direct scrollTop to avoid scrollIntoView
  // bubbling up to the document root and shifting the entire layout
  useEffect(() => {
    const el = transcriptRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [messages])

  // Refocus input when Aria finishes responding (done, error, or confirm card)
  useEffect(() => {
    if (!busy) {
      inputRef.current?.focus()
    }
  }, [busy])

  return (
    <div style={{ width: '100%', height: '100%', display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
      {/* Glass panel */}
      <div style={{
        flex: 1, minHeight: 0,
        background: 'rgba(17, 22, 31, 0.65)',
        backdropFilter: 'blur(28px)',
        WebkitBackdropFilter: 'blur(28px)',
        border: '1px solid rgba(58, 138, 170, 0.18)',
        borderRadius: 16,
        boxShadow: '0 8px 48px rgba(58, 138, 170, 0.08), 0 2px 24px rgba(0, 0, 0, 0.50)',
        overflow: 'hidden',
        display: 'flex', flexDirection: 'column',
        position: 'relative',
      }}>

        {/* Top-edge highlight */}
        <div style={{
          position: 'absolute', top: 0, left: 0, right: 0, height: 1,
          background: 'linear-gradient(90deg, transparent, rgba(134,213,242,0.38), transparent)',
          zIndex: 1, pointerEvents: 'none',
        }} />

        {/* Transcript / greeting */}
        {messages.length === 0 ? (
          <motion.div
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.45, delay: 0.25 }}
            style={{ flex: 1, display: 'flex', flexDirection: 'column',
              justifyContent: 'center', padding: '24px 20px' }}
          >
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start' }}>
              <span style={{
                fontSize: 10, color: C_MUTED, letterSpacing: '0.08em',
                marginBottom: 4, userSelect: 'none',
              }}>aria</span>
              <div style={{
                background: 'rgba(58, 138, 170, 0.07)',
                padding: '10px 14px',
                borderRadius: '12px 12px 12px 2px',
                fontSize: 14, lineHeight: 1.6, color: C_TEXT,
                maxWidth: '85%',
              }}>
                {greeting()}
              </div>
            </div>
          </motion.div>
        ) : (
          <div ref={transcriptRef} className="transcript-scroll" style={{
            flex: 1, overflowY: 'auto', minHeight: 0,
            padding: '24px 20px 12px',
            display: 'flex', flexDirection: 'column', gap: 18,
          }}>
            <AnimatePresence initial={false}>
              {messages.map(m => {

                // ── Confirmation card ────────────────────────────────────────
                if (m.confirmRequest) {
                  const cr = m.confirmRequest
                  const resolved = cr.chosen != null
                  return (
                    <motion.div
                      key={m.id}
                      initial={{ opacity: 0, scale: 0.96, y: 10 }}
                      animate={{ opacity: resolved ? 0.5 : 1, scale: 1, y: 0 }}
                      transition={{ duration: 0.25, ease: 'easeOut' }}
                      style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start' }}
                    >
                      <span style={{
                        fontSize: 10, color: C_AMBER_DIM, letterSpacing: '0.08em',
                        marginBottom: 4, userSelect: 'none',
                      }}>
                        aria · confirmation required
                      </span>
                      <div style={{
                        background: C_AMBER_GHOST,
                        border: `1px solid ${C_AMBER_DIM}`,
                        borderRadius: '12px 12px 12px 2px',
                        padding: '12px 14px', maxWidth: '85%',
                        boxShadow: '0 0 18px rgba(212, 165, 116, 0.07)',
                      }}>
                        <p style={{
                          margin: '0 0 12px 0', fontSize: 13, lineHeight: 1.55,
                          color: C_AMBER_TEXT,
                        }}>
                          {cr.actionDescription}
                        </p>
                        <div style={{ display: 'flex', gap: 8 }}>
                          <motion.button
                            whileTap={{ scale: 0.94 }}
                            disabled={resolved || busy}
                            onClick={() => resolveConfirm(m.id, 'confirmed')}
                            style={{
                              padding: '5px 14px', borderRadius: 7,
                              fontSize: 12, fontFamily: 'inherit',
                              cursor: resolved || busy ? 'default' : 'pointer',
                              background: resolved
                                ? (cr.chosen === 'confirmed' ? 'rgba(212,165,116,0.25)' : 'transparent')
                                : 'rgba(212,165,116,0.18)',
                              border: `1px solid ${resolved && cr.chosen !== 'confirmed' ? 'transparent' : C_AMBER_DIM}`,
                              color: resolved && cr.chosen !== 'confirmed' ? 'transparent' : C_AMBER,
                              transition: 'all 0.18s ease',
                            }}
                          >
                            {cr.chosen === 'confirmed' ? 'Confirmed ✓' : 'Confirm'}
                          </motion.button>
                          <motion.button
                            whileTap={{ scale: 0.94 }}
                            disabled={resolved || busy}
                            onClick={() => resolveConfirm(m.id, 'declined')}
                            style={{
                              padding: '5px 14px', borderRadius: 7,
                              fontSize: 12, fontFamily: 'inherit',
                              cursor: resolved || busy ? 'default' : 'pointer',
                              background: resolved && cr.chosen === 'declined'
                                ? 'rgba(130,130,130,0.15)' : 'transparent',
                              border: `1px solid ${resolved && cr.chosen !== 'declined' ? 'transparent' : 'rgba(150,150,150,0.30)'}`,
                              color: resolved && cr.chosen !== 'declined' ? 'transparent' : 'rgba(180,180,180,0.62)',
                              transition: 'all 0.18s ease',
                            }}
                          >
                            {cr.chosen === 'declined' ? 'Declined' : 'Cancel'}
                          </motion.button>
                        </div>
                      </div>
                    </motion.div>
                  )
                }

                // ── Regular message ──────────────────────────────────────────
                return (
                  <motion.div
                    key={m.id}
                    initial={{ opacity: 0, y: 12 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.25, ease: 'easeOut' }}
                    style={{
                      display: 'flex', flexDirection: 'column',
                      alignItems: m.role === 'user' ? 'flex-end' : 'flex-start',
                    }}
                  >
                    <span style={{
                      fontSize: 10, color: C_MUTED, letterSpacing: '0.08em',
                      marginBottom: 4, userSelect: 'none',
                    }}>
                      {m.role === 'user' ? 'you' : 'aria'}
                    </span>

                    {m.role === 'user' ? (
                      <div style={{
                        background: 'rgba(91, 168, 200, 0.14)',
                        padding: '10px 14px',
                        borderRadius: '12px 12px 2px 12px',
                        maxWidth: '85%', fontSize: 14, lineHeight: 1.6,
                        color: C_TEXT, wordBreak: 'break-word',
                      }}>
                        {m.content}
                      </div>
                    ) : (
                      <AriaBubble
                        content={m.content}
                        streaming={m.streaming}
                        error={m.error}
                      />
                    )}
                  </motion.div>
                )
              })}
            </AnimatePresence>
          </div>
        )}

        {/* Tool indicator */}
        <AnimatePresence>
          {currentTool && (
            <motion.div
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.18 }}
              style={{
                flexShrink: 0, display: 'flex',
                justifyContent: 'center', padding: '6px 0 2px',
              }}
            >
              <ToolPill toolName={currentTool} />
            </motion.div>
          )}
        </AnimatePresence>

        {/* Divider */}
        <div style={{ height: 1, background: 'rgba(58, 138, 170, 0.07)', flexShrink: 0 }} />

        {/* Input row */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 10,
          padding: '12px 14px 16px 18px', flexShrink: 0,
        }}>
          <input
            ref={inputRef}
            className="chat-input"
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && !busy && submit()}
            placeholder="talk to aria..."
            disabled={busy}
            autoFocus
            style={{
              flex: 1, background: 'transparent', border: 'none', outline: 'none',
              color: C_TEXT, fontSize: 14, fontFamily: 'inherit',
              minWidth: 0,
            }}
          />
          <button
            onClick={submit}
            disabled={!hasText || busy}
            style={{
              width: 30, height: 30, borderRadius: '50%',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              flexShrink: 0,
              cursor: hasText && !busy ? 'pointer' : 'default',
              background: hasText && !busy ? 'rgba(58,138,170,0.22)' : 'rgba(58,138,170,0.05)',
              border: `1px solid ${hasText && !busy ? 'rgba(58,138,170,0.55)' : 'rgba(58,138,170,0.12)'}`,
              boxShadow: hasText && !busy ? '0 0 14px rgba(58,138,170,0.28)' : 'none',
              transition: 'background 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease',
              color: hasText && !busy ? C_PEAK : 'rgba(58,138,170,0.25)',
            }}
          >
            <ArrowUp size={14} strokeWidth={2.5} />
          </button>
        </div>
      </div>
    </div>
  )
}
