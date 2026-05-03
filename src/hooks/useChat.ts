import { useState, useCallback, useRef } from 'react'
import type { AriaState } from './useAriaState'

export interface ChatMessage {
  id: string
  role: 'user' | 'aria'
  content: string
  streaming?: boolean
}

const MOCK_RESPONSE = "I hear you. The system isn't connected yet — I'm just the shell for now."

function sleep(ms: number): Promise<void> {
  return new Promise(r => setTimeout(r, ms))
}

export function useChat(onStateChange: (s: AriaState) => void) {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const onStateChangeRef = useRef(onStateChange)
  onStateChangeRef.current = onStateChange

  const submit = useCallback(async () => {
    const text = input.trim()
    if (!text || busy) return

    setBusy(true)
    setInput('')
    setMessages(prev => [...prev, { id: `u-${Date.now()}`, role: 'user', content: text }])

    onStateChangeRef.current('thinking')
    await sleep(1200)

    onStateChangeRef.current('speaking')
    const ariaId = `a-${Date.now()}`
    setMessages(prev => [...prev, { id: ariaId, role: 'aria', content: '', streaming: true }])

    for (let i = 1; i <= MOCK_RESPONSE.length; i++) {
      await sleep(30)
      setMessages(prev =>
        prev.map(m => m.id === ariaId ? { ...m, content: MOCK_RESPONSE.slice(0, i) } : m)
      )
    }

    setMessages(prev => prev.map(m => m.id === ariaId ? { ...m, streaming: false } : m))
    onStateChangeRef.current('idle')
    setBusy(false)
  }, [input, busy])

  return { messages, input, setInput, submit, busy }
}
