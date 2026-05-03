import { useRef, useEffect } from 'react'
import { ArrowUp } from 'lucide-react'
import type { AriaState } from '../hooks/useAriaState'
import { useChat } from '../hooks/useChat'

interface Props {
  onStateChange: (s: AriaState) => void
}

const C_BASE = '#3A8AAA'
const C_PEAK = '#86D5F2'
const C_TEXT = '#C8E8F4'
const C_MUTED = 'rgba(58, 138, 170, 0.5)'

export default function ChatPanel({ onStateChange }: Props) {
  const { messages, input, setInput, submit, busy } = useChat(onStateChange)
  const bottomRef = useRef<HTMLDivElement>(null)
  const hasText = input.trim().length > 0

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  return (
    // Outer: fills column width, sets panel height
    <div style={{
      width: '100%',
      height: '75vh',
      display: 'flex',
      flexDirection: 'column',
    }}>
      {/* Glass panel — full height, flex column */}
      <div style={{
        flex: 1,
        minHeight: 0,
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

        {/* Inner top-edge highlight */}
        <div style={{
          position: 'absolute',
          top: 0, left: 0, right: 0,
          height: 1,
          background: 'linear-gradient(90deg, transparent, rgba(134, 213, 242, 0.45), transparent)',
          zIndex: 1,
          pointerEvents: 'none',
        }} />

        {/* Transcript area — always present, fills remaining space */}
        {messages.length === 0 ? (
          <div style={{
            flex: 1,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}>
            <span style={{
              color: 'rgba(58, 138, 170, 0.28)',
              fontStyle: 'italic',
              fontSize: 14,
              letterSpacing: '0.01em',
              userSelect: 'none',
            }}>
              talk to aria...
            </span>
          </div>
        ) : (
          <div
            className="transcript-scroll"
            style={{
              flex: 1,
              overflowY: 'auto',
              minHeight: 0,
              padding: '24px 20px 12px',
              display: 'flex',
              flexDirection: 'column',
              gap: 16,
            }}
          >
            {messages.map(m => (
              <div key={m.id} style={{
                display: 'flex',
                flexDirection: 'column',
                alignItems: m.role === 'user' ? 'flex-end' : 'flex-start',
              }}>
                <span style={{
                  fontSize: 10,
                  color: C_MUTED,
                  letterSpacing: '0.08em',
                  marginBottom: 4,
                  userSelect: 'none',
                }}>
                  {m.role === 'user' ? 'you' : 'aria'}
                </span>
                <div style={{
                  background: m.role === 'user'
                    ? 'rgba(91, 168, 200, 0.15)'
                    : 'rgba(58, 138, 170, 0.08)',
                  padding: '9px 13px',
                  borderRadius: m.role === 'user'
                    ? '12px 12px 2px 12px'
                    : '12px 12px 12px 2px',
                  maxWidth: '85%',
                  fontSize: 14,
                  lineHeight: 1.6,
                  color: C_TEXT,
                  wordBreak: 'break-word',
                }}>
                  {m.content}
                  {m.streaming && (
                    <span
                      className="cursor-blink"
                      style={{
                        display: 'inline-block',
                        width: 1.5,
                        height: '1em',
                        background: C_PEAK,
                        marginLeft: 2,
                        verticalAlign: 'text-bottom',
                        borderRadius: 1,
                      }}
                    />
                  )}
                </div>
              </div>
            ))}
            <div ref={bottomRef} />
          </div>
        )}

        {/* Divider */}
        <div style={{
          height: 1,
          background: 'rgba(58, 138, 170, 0.08)',
          flexShrink: 0,
        }} />

        {/* Input row */}
        <div style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          padding: '13px 14px 13px 18px',
          flexShrink: 0,
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
              flex: 1,
              background: 'transparent',
              border: 'none',
              outline: 'none',
              color: C_TEXT,
              fontSize: 14,
              fontFamily: 'inherit',
              caretColor: C_BASE,
              minWidth: 0,
            }}
          />

          <button
            onClick={submit}
            disabled={!hasText || busy}
            style={{
              width: 30,
              height: 30,
              borderRadius: '50%',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
              cursor: hasText && !busy ? 'pointer' : 'default',
              background: hasText && !busy
                ? 'rgba(58, 138, 170, 0.22)'
                : 'rgba(58, 138, 170, 0.06)',
              border: `1px solid ${hasText && !busy
                ? 'rgba(58, 138, 170, 0.55)'
                : 'rgba(58, 138, 170, 0.14)'}`,
              boxShadow: hasText && !busy
                ? '0 0 14px rgba(58, 138, 170, 0.30)'
                : 'none',
              transition: 'background 0.2s ease, border-color 0.2s ease, box-shadow 0.2s ease',
              color: hasText && !busy ? C_PEAK : 'rgba(58, 138, 170, 0.30)',
            }}
          >
            <ArrowUp size={14} strokeWidth={2.5} />
          </button>
        </div>
      </div>
    </div>
  )
}
