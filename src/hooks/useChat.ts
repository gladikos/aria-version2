import { useState, useCallback, useRef } from 'react'
import type { AriaState } from './useAriaState'
import { sendMessage } from '../lib/aria'
import type { ConfirmPayload } from '../lib/aria'

export interface ConfirmRequest {
  actionDescription: string
  toolName: string
  toolArgs: unknown
  chosen?: 'confirmed' | 'declined'
}

export interface ChatMessage {
  id: string
  role: 'user' | 'aria'
  content: string
  streaming?: boolean
  error?: boolean
  confirmRequest?: ConfirmRequest
}

export function useChat(onStateChange: (s: AriaState) => void, initialMessages: ChatMessage[] = []) {
  const [messages, setMessages] = useState<ChatMessage[]>(initialMessages)
  const msgsRef = useRef<ChatMessage[]>(initialMessages)
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const [currentTool, setCurrentTool] = useState<string | null>(null)
  const stateRef = useRef(onStateChange)
  stateRef.current = onStateChange

  const setMsgs = useCallback((next: ChatMessage[]) => {
    msgsRef.current = next
    setMessages(next)
  }, [])

  // Core submit logic shared by submit() and submitMessage()
  const doSubmit = useCallback((text: string) => {
    if (!text || busy) return

    setBusy(true)
    setInput('')

    const userMsg: ChatMessage = { id: `u-${Date.now()}`, role: 'user', content: text }
    const history = [...msgsRef.current, userMsg]
    setMsgs(history)
    stateRef.current('thinking')

    // Build API messages: confirmRequest cards get a synthetic assistant turn so the
    // history alternates user/assistant correctly for the Anthropic API.
    const apiMessages = history
      .map(m => {
        if (m.confirmRequest) {
          return {
            role: 'assistant' as const,
            content: `I need your confirmation before proceeding. Pending action: ${m.confirmRequest.actionDescription}`,
          }
        }
        return {
          role: (m.role === 'aria' ? 'assistant' : 'user') as 'user' | 'assistant',
          content: m.content,
        }
      })
      .filter(m => m.content.trim() !== '')

    const ariaId = `a-${Date.now()}`
    let firstToken = true

    sendMessage(apiMessages, {
      onToken: (token) => {
        if (firstToken) {
          firstToken = false
          console.log('[aria] first token received — streaming started')
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
        firstToken = true
        setMsgs(msgsRef.current.filter(m => m.id !== ariaId))
        setCurrentTool(null)
        stateRef.current('thinking')
        console.warn('[aria] grounding retry — discarded streamed response')
      },
      onConfirmRequest: (payload: ConfirmPayload) => {
        const confirmId = `confirm-${Date.now()}`
        setMsgs([...msgsRef.current, {
          id: confirmId,
          role: 'aria',
          content: '',
          confirmRequest: {
            actionDescription: payload.action_description,
            toolName: payload.tool_name,
            toolArgs: payload.tool_args,
          },
        }])
        setCurrentTool(null)
        stateRef.current('idle')
        setBusy(false)
      },
    })
  }, [busy, setMsgs])

  const submit = useCallback(() => {
    doSubmit(input.trim())
  }, [input, doSubmit])

  // Called by confirm/cancel buttons
  const submitMessage = useCallback((text: string) => {
    doSubmit(text)
  }, [doSubmit])

  // Mark a confirm card as resolved (chosen) and submit the response
  const resolveConfirm = useCallback((msgId: string, choice: 'confirmed' | 'declined') => {
    setMsgs(msgsRef.current.map(m =>
      m.id === msgId && m.confirmRequest
        ? { ...m, confirmRequest: { ...m.confirmRequest, chosen: choice } }
        : m
    ))
    submitMessage(choice === 'confirmed' ? 'Yes, go ahead.' : "No, don't do that.")
  }, [setMsgs, submitMessage])

  return { messages, input, setInput, submit, submitMessage, resolveConfirm, busy, currentTool }
}
