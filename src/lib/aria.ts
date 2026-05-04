import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'

export interface ApiMessage {
  role: 'user' | 'assistant'
  content: string
}

export interface ConfirmPayload {
  action_description: string
  tool_name: string
  tool_args: unknown
}

interface ToolStartPayload {
  tool_name: string
  tool_args_summary: string
}

export interface ScreenshotPayload {
  image_base64: string
  width: number
  height: number
}

interface Callbacks {
  onToken: (token: string) => void
  onDone: () => void
  onError: (error: string) => void
  onTool?: (name: string, summary: string) => void
  onToolEnd?: () => void
  onResetStream?: () => void
  onConfirmRequest?: (payload: ConfirmPayload) => void
  onScreenshot?: (payload: ScreenshotPayload) => void
}

export function sendMessage(messages: ApiMessage[], callbacks: Callbacks): void {
  const unlisteners: Array<() => void> = []

  function cleanup() {
    unlisteners.forEach(fn => fn())
    unlisteners.length = 0
  }

  Promise.all([
    listen<string>            ('aria-token',               e => callbacks.onToken(e.payload)),
    listen<void>              ('aria-done',                () => { cleanup(); callbacks.onDone() }),
    listen<string>            ('aria-error',               e => { cleanup(); callbacks.onError(e.payload) }),
    listen<ToolStartPayload>  ('aria-tool-start',          e => callbacks.onTool?.(e.payload.tool_name, e.payload.tool_args_summary)),
    listen<void>              ('aria-tool-end',            () => callbacks.onToolEnd?.()),
    listen<void>              ('aria-reset-stream',        () => callbacks.onResetStream?.()),
    listen<ConfirmPayload>    ('aria-confirm-request',     e => { cleanup(); callbacks.onConfirmRequest?.(e.payload) }),
    listen<ScreenshotPayload> ('aria-screenshot-captured', e => callbacks.onScreenshot?.(e.payload)),
  ]).then(([unToken, unDone, unError, unToolStart, unToolEnd, unReset, unConfirm, unScreenshot]) => {
    unlisteners.push(unToken, unDone, unError, unToolStart, unToolEnd, unReset, unConfirm, unScreenshot)

    console.log('[aria] sending messages to Rust:', JSON.stringify(messages, null, 2))
    invoke('chat_stream', { messages }).catch(e => {
      cleanup()
      callbacks.onError(String(e))
    })
  }).catch(e => {
    callbacks.onError(`Failed to set up listeners: ${e}`)
  })
}
