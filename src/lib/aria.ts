import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'

export interface ApiMessage {
  role: 'user' | 'assistant'
  content: string
}

interface Callbacks {
  onToken: (token: string) => void
  onDone: () => void
  onError: (error: string) => void
}

export function sendMessage(messages: ApiMessage[], callbacks: Callbacks): void {
  const unlisteners: Array<() => void> = []

  function cleanup() {
    unlisteners.forEach(fn => fn())
    unlisteners.length = 0
  }

  // Set up all listeners before invoking so no events are missed
  Promise.all([
    listen<string>('aria-token', e => callbacks.onToken(e.payload)),
    listen<void>('aria-done', () => { cleanup(); callbacks.onDone() }),
    listen<string>('aria-error', e => { cleanup(); callbacks.onError(e.payload) }),
  ]).then(([unToken, unDone, unError]) => {
    unlisteners.push(unToken, unDone, unError)

    invoke('chat_stream', { messages }).catch(e => {
      cleanup()
      callbacks.onError(String(e))
    })
  }).catch(e => {
    callbacks.onError(`Failed to set up listeners: ${e}`)
  })
}
