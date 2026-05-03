import { useState, useCallback, useRef } from 'react'
import type { AriaState } from './useAriaState'
import { sendMessage } from '../lib/aria'

export interface ChatMessage {
  id: string
  role: 'user' | 'aria'
  content: string
  streaming?: boolean
  error?: boolean
}

export function useChat(onStateChange: (s: AriaState) => void) {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const msgsRef = useRef<ChatMessage[]>([])
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const [currentTool, setCurrentTool] = useState<string | null>(null)
  const stateRef = useRef(onStateChange)
  stateRef.current = onStateChange

  const setMsgs = useCallback((next: ChatMessage[]) => {
    msgsRef.current = next
    setMessages(next)
  }, [])

  const submit = useCallback(() => {
    const text = input.trim()
    if (!text || busy) return

    setBusy(true)
    setInput('')

    const userMsg: ChatMessage = { id: `u-${Date.now()}`, role: 'user', content: text }
    const history = [...msgsRef.current, userMsg]
    setMsgs(history)
    stateRef.current('thinking')

    const apiMessages = history.map(m => ({
      role: (m.role === 'aria' ? 'assistant' : 'user') as 'user' | 'assistant',
      content: m.content,
    }))

    const ariaId = `a-${Date.now()}`
    // firstToken is a plain let — both onToken and onResetStream close over the
    // same variable, so onResetStream can reset it for a grounding retry.
    let firstToken = true

    sendMessage(apiMessages, {
      onToken: (token) => {
        if (firstToken) {
          firstToken = false
          stateRef.current('speaking')
          setCurrentTool(null)
          setMsgs([...msgsRef.current, { id: ariaId, role: 'aria', content: token, streaming: true }])
        } else {
          setMsgs(msgsRef.current.map(m =>
            m.id === ariaId ? { ...m, content: m.content + token } : m
          ))
        }
      },
      onDone: () => {
        setMsgs(msgsRef.current.map(m => m.id === ariaId ? { ...m, streaming: false } : m))
        setCurrentTool(null)
        stateRef.current('idle')
        setBusy(false)
      },
      onError: (error) => {
        const withoutPartial = msgsRef.current.filter(m => m.id !== ariaId)
        setMsgs([...withoutPartial, {
          id: ariaId, role: 'aria', content: error, streaming: false, error: true,
        }])
        setCurrentTool(null)
        stateRef.current('idle')
        setBusy(false)
      },
      onTool: (toolName) => {
        setCurrentTool(toolName)
      },
      onResetStream: () => {
        // Grounding retry: discard the hallucinated streaming message and reset
        // so the next onToken creates a fresh bubble
        firstToken = true
        setMsgs(msgsRef.current.filter(m => m.id !== ariaId))
        setCurrentTool(null)
        stateRef.current('thinking')
        console.warn('[aria] grounding retry — discarded streamed response, asking model to use tools')
      },
    })
  }, [input, busy, setMsgs])

  return { messages, input, setInput, submit, busy, currentTool }
}
