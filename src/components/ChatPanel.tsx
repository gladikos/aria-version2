import { useRef, useEffect, useState, useCallback, createContext, useContext } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Components } from 'react-markdown'
import {
  ArrowUp, Search, Globe, Folder, FileText, Pencil,
  FolderPlus, Copy, Trash2, ExternalLink,
  Terminal, Info, AlertCircle, Check, MoveRight,
  MousePointer2, Keyboard, Timer, Camera, ChevronsDown, Bookmark, BookmarkX, Printer, Mic, MicOff, Volume2,
  Music, Pause, Play, SkipForward,
} from 'lucide-react'
import type { AriaState } from '../hooks/useAriaState'
import { useChat, type ChatMessage } from '../hooks/useChat'
import { useVoice } from '../contexts/VoiceContext'

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
const TOOL_META: Record<string, { icon: React.ReactNode }> = {
  web_search:           { icon: <Search size={10} />        },
  fetch_url:            { icon: <Globe size={10} />          },
  list_directory:       { icon: <Folder size={10} />         },
  read_file:            { icon: <FileText size={10} />       },
  write_file:           { icon: <Pencil size={10} />         },
  search_filesystem:    { icon: <Search size={10} />         },
  create_directory:     { icon: <FolderPlus size={10} />     },
  move_path:            { icon: <MoveRight size={10} />      },
  copy_path:            { icon: <Copy size={10} />           },
  delete_path:          { icon: <Trash2 size={10} />         },
  open_in_app:          { icon: <ExternalLink size={10} />   },
  run_command:          { icon: <Terminal size={10} />       },
  get_path_info:        { icon: <Info size={10} />           },
  request_confirmation: { icon: <AlertCircle size={10} />    },
  browser_navigate:     { icon: <Globe size={10} />          },
  browser_type:         { icon: <Keyboard size={10} />       },
  browser_click:        { icon: <MousePointer2 size={10} />  },
  browser_wait:         { icon: <Timer size={10} />          },
  browser_scroll:       { icon: <ChevronsDown size={10} />   },
  browser_get_text:     { icon: <FileText size={10} />       },
  browser_screenshot:   { icon: <Camera size={10} />         },
  browser_current_url:  { icon: <Globe size={10} />          },
  launch_aria_chrome:   { icon: <Globe size={10} />          },
  launch_app:           { icon: <ExternalLink size={10} />   },
  remember:             { icon: <Bookmark size={10} />        },
  forget:               { icon: <BookmarkX size={10} />       },
  print_file:           { icon: <Printer size={10} />         },
  convert_to_pdf:       { icon: <FileText size={10} />        },
  take_screenshot:      { icon: <Camera size={10} />          },
  set_voice_mode:       { icon: <Volume2 size={10} />         },
  spotify_play:         { icon: <Music size={10} />           },
  spotify_pause:        { icon: <Pause size={10} />           },
  spotify_resume:       { icon: <Play size={10} />            },
  spotify_skip_next:    { icon: <SkipForward size={10} />     },
  spotify_current_track:{ icon: <Music size={10} />           },
}

function toolActionLabel(name: string, summary: string): string {
  const trunc = (s: string, n = 38) => s.length > n ? s.slice(0, n) + '…' : s
  const q = (s: string) => s ? `"${trunc(s, 30)}"` : ''
  let host = ''
  try { host = new URL(summary).hostname.replace(/^www\./, '') } catch { host = '' }

  switch (name) {
    case 'web_search':           return `Searching the web for ${q(summary)}`
    case 'fetch_url':            return `Fetching ${host || trunc(summary)}`
    case 'browser_navigate':     return `Opening ${host || trunc(summary)}`
    case 'browser_type':         return `Typing ${q(summary)}`
    case 'browser_click':        return 'Clicking first result'
    case 'browser_wait':         return 'Waiting for page…'
    case 'browser_scroll':       return `Scrolling ${summary}`
    case 'browser_get_text':     return 'Reading page content'
    case 'browser_screenshot':   return 'Taking screenshot'
    case 'browser_current_url':  return 'Checking current URL'
    case 'launch_app':           return `Opening ${trunc(summary)}`
    case 'launch_aria_chrome':   return 'Starting Aria-Chrome'
    case 'list_directory':       return `Reading ${trunc(summary)}`
    case 'read_file':            return `Reading ${trunc(summary)}`
    case 'write_file':           return `Writing ${trunc(summary)}`
    case 'search_filesystem':    return `Searching for ${q(summary)}`
    case 'create_directory':     return `Creating ${trunc(summary)}`
    case 'move_path':            return `Moving ${trunc(summary)}`
    case 'copy_path':            return `Copying ${trunc(summary)}`
    case 'delete_path':          return `Deleting ${trunc(summary)}`
    case 'open_in_app':          return `Opening ${trunc(summary)}`
    case 'run_command':          return `Running ${summary}`
    case 'get_path_info':        return `Checking ${trunc(summary)}`
    case 'remember':             return `Remembering ${q(summary)}`
    case 'forget':               return `Forgetting ${q(summary)}`
    case 'print_file':           return `Printing ${trunc(summary)}`
    case 'convert_to_pdf':       return `Converting ${trunc(summary)} to PDF`
    case 'take_screenshot':      return summary === 'clipboard' ? 'Capturing screen' : `Saving screenshot to ${trunc(summary)}`
    case 'set_voice_mode':        return summary === 'ON' ? 'Enabling voice mode' : 'Disabling voice mode'
    case 'spotify_play':          return `Playing ${q(summary)} on Spotify`
    case 'spotify_pause':         return 'Pausing Spotify'
    case 'spotify_resume':        return 'Resuming Spotify'
    case 'spotify_skip_next':     return 'Skipping to next track'
    case 'spotify_current_track': return 'Checking what\'s playing'
    case 'request_confirmation': return 'Requesting confirmation'
    default:                     return name.replace(/_/g, ' ')
  }
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

// ─── Screenshot bubble ────────────────────────────────────────────────────────
function ScreenshotBubble({ screenshot }: {
  screenshot: { dataUrl: string; width: number; height: number }
}) {
  const [expanded, setExpanded] = useState(false)
  return (
    <>
      <div style={{
        background: 'rgba(58, 138, 170, 0.08)',
        padding: '8px 10px',
        borderRadius: '12px 12px 12px 2px',
        maxWidth: 500,
      }}>
        <img
          src={screenshot.dataUrl}
          alt="Screenshot"
          onClick={() => setExpanded(true)}
          style={{
            display: 'block', width: '100%', maxWidth: 480,
            borderRadius: 6, cursor: 'zoom-in',
            border: '1px solid rgba(58,138,170,0.22)',
          }}
        />
        <div style={{ fontSize: 11, color: C_MUTED, marginTop: 5, letterSpacing: '0.02em' }}>
          {screenshot.width}×{screenshot.height} · click to expand
        </div>
      </div>

      <AnimatePresence>
        {expanded && (
          <motion.div
            key="screenshot-modal"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            onClick={() => setExpanded(false)}
            style={{
              position: 'fixed', inset: 0, zIndex: 9999,
              background: 'rgba(0,0,0,0.88)',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              cursor: 'zoom-out',
            }}
          >
            <img
              src={screenshot.dataUrl}
              alt="Screenshot full size"
              onClick={e => e.stopPropagation()}
              style={{
                maxWidth: '92vw', maxHeight: '92vh',
                borderRadius: 8, cursor: 'default',
                boxShadow: '0 8px 64px rgba(0,0,0,0.6)',
              }}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </>
  )
}

// ─── Tool status pill ─────────────────────────────────────────────────────────
function ToolPill({ tool }: { tool: { name: string; summary: string } }) {
  const meta = TOOL_META[tool.name] ?? { icon: <Info size={10} /> }
  const label = toolActionLabel(tool.name, tool.summary)
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 8,
      padding: '7px 18px',
      background: 'rgba(58,138,170,0.055)',
      borderTop: '1px solid rgba(58,138,170,0.10)',
      fontSize: 12, color: 'rgba(134,213,242,0.62)',
      letterSpacing: '0.015em',
    }}>
      <motion.span
        animate={{ opacity: [0.35, 1, 0.35] }}
        transition={{ duration: 1.4, repeat: Infinity, ease: 'easeInOut' }}
        style={{
          display: 'inline-block', width: 6, height: 6,
          borderRadius: '50%', background: C_BASE, flexShrink: 0,
        }}
      />
      <span style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
        {meta.icon}
        {label}
      </span>
    </div>
  )
}

// ─── Main component ───────────────────────────────────────────────────────────
interface Props {
  onStateChange:    (s: AriaState) => void
  initialMessages?: ChatMessage[]
  chatId:           string
  onTitleGenerated?: (title: string) => void
}

export default function ChatPanel({ onStateChange, initialMessages = [], chatId, onTitleGenerated }: Props) {
  const { messages, input, setInput, submit, resolveConfirm, busy, currentTool } = useChat(onStateChange, initialMessages, chatId, onTitleGenerated ?? null)
  const { voiceEnabled, isListening, toggleVoice } = useVoice()
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

                // ── Screenshot bubble ────────────────────────────────────────
                if (m.screenshot) {
                  return (
                    <motion.div
                      key={m.id}
                      initial={{ opacity: 0, y: 12 }}
                      animate={{ opacity: 1, y: 0 }}
                      transition={{ duration: 0.25, ease: 'easeOut' }}
                      style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start' }}
                    >
                      <span style={{
                        fontSize: 10, color: C_MUTED, letterSpacing: '0.08em',
                        marginBottom: 4, userSelect: 'none',
                      }}>aria</span>
                      <ScreenshotBubble screenshot={m.screenshot} />
                    </motion.div>
                  )
                }

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

        {/* Divider */}
        <div style={{ height: 1, background: 'rgba(58, 138, 170, 0.07)', flexShrink: 0 }} />

        {/* Live tool status pill — sits above input, full width */}
        <AnimatePresence>
          {currentTool && (
            <motion.div
              key="tool-pill"
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: 'auto' }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.18, ease: 'easeOut' }}
              style={{ flexShrink: 0, overflow: 'hidden' }}
            >
              <ToolPill tool={currentTool} />
            </motion.div>
          )}
        </AnimatePresence>

        {/* Input row */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 10,
          padding: '12px 14px 16px 18px', flexShrink: 0,
        }}>
          {/* Listening pulse indicator */}
          <AnimatePresence>
            {isListening && (
              <motion.span
                key="listening-dot"
                initial={{ opacity: 0, scale: 0.6 }}
                animate={{ opacity: [0.4, 1, 0.4], scale: 1 }}
                exit={{ opacity: 0, scale: 0.6 }}
                transition={{ duration: 1.2, repeat: Infinity, ease: 'easeInOut' }}
                title="Listening…"
                style={{
                  display: 'inline-block', width: 7, height: 7,
                  borderRadius: '50%', background: 'rgba(220,80,80,0.9)',
                  flexShrink: 0,
                }}
              />
            )}
          </AnimatePresence>

          <input
            ref={inputRef}
            className="chat-input"
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && !busy && submit()}
            placeholder={isListening ? 'listening…' : 'talk to aria...'}
            disabled={busy}
            autoFocus
            style={{
              flex: 1, background: 'transparent', border: 'none', outline: 'none',
              color: C_TEXT, fontSize: 14, fontFamily: 'inherit',
              minWidth: 0,
            }}
          />

          {/* Voice toggle button */}
          <motion.button
            whileTap={{ scale: 0.9 }}
            onClick={toggleVoice}
            title={voiceEnabled ? 'Voice on — click to disable (Ctrl+Space to record)' : 'Enable voice mode'}
            style={{
              width: 30, height: 30, borderRadius: '50%',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              flexShrink: 0, cursor: 'pointer',
              background: voiceEnabled ? 'rgba(100,200,120,0.18)' : 'rgba(58,138,170,0.05)',
              border: `1px solid ${voiceEnabled ? 'rgba(100,200,120,0.45)' : 'rgba(58,138,170,0.12)'}`,
              boxShadow: voiceEnabled ? '0 0 12px rgba(100,200,120,0.22)' : 'none',
              transition: 'background 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease',
              color: voiceEnabled ? 'rgba(130,220,140,0.9)' : 'rgba(58,138,170,0.30)',
            }}
          >
            {voiceEnabled ? <Mic size={13} strokeWidth={2} /> : <MicOff size={13} strokeWidth={2} />}
          </motion.button>

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
