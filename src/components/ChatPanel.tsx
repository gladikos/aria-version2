import { useRef, useEffect } from 'react'
import { ArrowUp } from 'lucide-react'
import type { AriaState } from '../hooks/useAriaState'
import { useChat } from '../hooks/useChat'

interface Props {
  onStateChange: (s: AriaState) => void
}

const C_BASE  = '#3A8AAA'
const C_PEAK  = '#86D5F2'
const C_TEXT  = '#C8E8F4'
const C_MUTED = 'rgba(58, 138, 170, 0.5)'

// Amber accent for the confirmation card
const C_AMBER       = '#d4a574'
const C_AMBER_DIM   = 'rgba(212, 165, 116, 0.45)'
const C_AMBER_GHOST = 'rgba(212, 165, 116, 0.12)'
const C_AMBER_TEXT  = 'rgba(240, 210, 170, 0.90)'

export default function ChatPanel({ onStateChange }: Props) {
  const { messages, input, setInput, submit, resolveConfirm, busy, currentTool } = useChat(onStateChange)
  const bottomRef = useRef<HTMLDivElement>(null)
  const hasText = input.trim().length > 0

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  return (
    <div style={{ width: '100%', height: '75vh', display: 'flex', flexDirection: 'column' }}>
      {/* Glass panel */}
      <div style={{
        flex: 1, minHeight: 0,
        background: 'rgba(17, 22, 31, 0.65)',
        backdropFilter: 'blur(28px)',
        WebkitBackdropFilter: 'blur(28px)',
        border: '1px solid rgba(58, 138, 170, 0.20)',
        borderRadius: 16,
        boxShadow: '0 8px 48px rgba(58, 138, 170, 0.10), 0 2px 24px rgba(0, 0, 0, 0.50)',
        overflow: 'hidden',
        display: 'flex',
        flexDirection: 'column',
        position: 'relative',
      }}>

        {/* Top-edge highlight */}
        <div style={{
          position: 'absolute', top: 0, left: 0, right: 0, height: 1,
          background: 'linear-gradient(90deg, transparent, rgba(134, 213, 242, 0.45), transparent)',
          zIndex: 1, pointerEvents: 'none',
        }} />

        {/* Transcript */}
        {messages.length === 0 ? (
          <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <span style={{
              color: 'rgba(58, 138, 170, 0.28)', fontStyle: 'italic',
              fontSize: 14, letterSpacing: '0.01em', userSelect: 'none',
            }}>
              talk to aria...
            </span>
          </div>
        ) : (
          <div className="transcript-scroll" style={{
            flex: 1, overflowY: 'auto', minHeight: 0,
            padding: '24px 20px 12px',
            display: 'flex', flexDirection: 'column', gap: 16,
          }}>
            {messages.map(m => {
              // ── Confirmation card ──────────────────────────────────────────
              if (m.confirmRequest) {
                const cr = m.confirmRequest
                const resolved = cr.chosen != null
                return (
                  <div key={m.id} style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start' }}>
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
                      padding: '12px 14px',
                      maxWidth: '85%',
                      boxShadow: `0 0 18px rgba(212, 165, 116, 0.08)`,
                    }}>
                      <p style={{
                        margin: '0 0 12px 0', fontSize: 13, lineHeight: 1.55,
                        color: C_AMBER_TEXT,
                      }}>
                        {cr.actionDescription}
                      </p>
                      <div style={{ display: 'flex', gap: 8 }}>
                        <button
                          disabled={resolved || busy}
                          onClick={() => resolveConfirm(m.id, 'confirmed')}
                          style={{
                            padding: '5px 14px', borderRadius: 7, fontSize: 12, fontFamily: 'inherit',
                            cursor: resolved || busy ? 'default' : 'pointer',
                            background: resolved
                              ? (cr.chosen === 'confirmed' ? 'rgba(212,165,116,0.25)' : 'transparent')
                              : 'rgba(212,165,116,0.18)',
                            border: `1px solid ${resolved && cr.chosen !== 'confirmed' ? 'transparent' : C_AMBER_DIM}`,
                            color: resolved && cr.chosen !== 'confirmed' ? 'transparent' : C_AMBER,
                            transition: 'all 0.15s ease',
                          }}
                        >
                          {cr.chosen === 'confirmed' ? 'Confirmed ✓' : 'Confirm'}
                        </button>
                        <button
                          disabled={resolved || busy}
                          onClick={() => resolveConfirm(m.id, 'declined')}
                          style={{
                            padding: '5px 14px', borderRadius: 7, fontSize: 12, fontFamily: 'inherit',
                            cursor: resolved || busy ? 'default' : 'pointer',
                            background: resolved && cr.chosen === 'declined'
                              ? 'rgba(130,130,130,0.15)' : 'transparent',
                            border: `1px solid ${resolved && cr.chosen !== 'declined' ? 'transparent' : 'rgba(150,150,150,0.35)'}`,
                            color: resolved && cr.chosen !== 'declined' ? 'transparent' : 'rgba(180,180,180,0.65)',
                            transition: 'all 0.15s ease',
                          }}
                        >
                          {cr.chosen === 'declined' ? 'Declined' : 'Cancel'}
                        </button>
                      </div>
                    </div>
                  </div>
                )
              }

              // ── Regular message ────────────────────────────────────────────
              return (
                <div key={m.id} style={{
                  display: 'flex', flexDirection: 'column',
                  alignItems: m.role === 'user' ? 'flex-end' : 'flex-start',
                }}>
                  <span style={{
                    fontSize: 10, color: C_MUTED, letterSpacing: '0.08em',
                    marginBottom: 4, userSelect: 'none',
                  }}>
                    {m.role === 'user' ? 'you' : 'aria'}
                  </span>
                  <div style={{
                    background: m.error
                      ? 'rgba(180, 70, 70, 0.12)'
                      : m.role === 'user'
                        ? 'rgba(91, 168, 200, 0.15)'
                        : 'rgba(58, 138, 170, 0.08)',
                    padding: '9px 13px',
                    borderRadius: m.role === 'user'
                      ? '12px 12px 2px 12px'
                      : '12px 12px 12px 2px',
                    maxWidth: '85%',
                    fontSize: 14, lineHeight: 1.6,
                    color: m.error ? 'rgba(220, 130, 130, 0.85)' : C_TEXT,
                    wordBreak: 'break-word',
                  }}>
                    {m.content}
                    {m.streaming && (
                      <span className="cursor-blink" style={{
                        display: 'inline-block', width: 1.5, height: '1em',
                        background: C_PEAK, marginLeft: 2,
                        verticalAlign: 'text-bottom', borderRadius: 1,
                      }} />
                    )}
                  </div>
                </div>
              )
            })}
            <div ref={bottomRef} />
          </div>
        )}

        {/* Tool indicator */}
        {currentTool && (
          <div style={{
            flexShrink: 0, textAlign: 'center', fontSize: 11, fontStyle: 'italic',
            color: 'rgba(58, 138, 170, 0.45)', padding: '6px 0 2px',
            letterSpacing: '0.04em', userSelect: 'none',
          }}>
            using {currentTool}...
          </div>
        )}

        {/* Divider */}
        <div style={{ height: 1, background: 'rgba(58, 138, 170, 0.08)', flexShrink: 0 }} />

        {/* Input row */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 10,
          padding: '13px 14px 13px 18px', flexShrink: 0,
        }}>
          <input
            className="chat-input"
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && submit()}
            placeholder="talk to aria..."
            disabled={busy}
            autoFocus
            style={{
              flex: 1, background: 'transparent', border: 'none', outline: 'none',
              color: C_TEXT, fontSize: 14, fontFamily: 'inherit',
              caretColor: C_BASE, minWidth: 0,
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
              background: hasText && !busy ? 'rgba(58,138,170,0.22)' : 'rgba(58,138,170,0.06)',
              border: `1px solid ${hasText && !busy ? 'rgba(58,138,170,0.55)' : 'rgba(58,138,170,0.14)'}`,
              boxShadow: hasText && !busy ? '0 0 14px rgba(58,138,170,0.30)' : 'none',
              transition: 'background 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease',
              color: hasText && !busy ? C_PEAK : 'rgba(58,138,170,0.30)',
            }}
          >
            <ArrowUp size={14} strokeWidth={2.5} />
          </button>
        </div>
      </div>
    </div>
  )
}
