import { createContext, useContext, useState, useEffect, useCallback } from 'react'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'

interface VoiceContextValue {
  voiceEnabled:          boolean
  isListening:           boolean
  pendingVoiceText:      string | null
  clearPendingVoiceText: () => void
  toggleVoice:           () => void
}

const VoiceContext = createContext<VoiceContextValue>({
  voiceEnabled:          false,
  isListening:           false,
  pendingVoiceText:      null,
  clearPendingVoiceText: () => {},
  toggleVoice:           () => {},
})

export function useVoice() {
  return useContext(VoiceContext)
}

export function VoiceProvider({ children }: { children: React.ReactNode }) {
  const [voiceEnabled,     setVoiceEnabled]     = useState(false)
  const [isListening,      setIsListening]      = useState(false)
  const [pendingVoiceText, setPendingVoiceText] = useState<string | null>(null)

  const toggleVoice = useCallback(() => {
    void invoke('set_voice_enabled', { enabled: !voiceEnabled })
  }, [voiceEnabled])

  const clearPendingVoiceText = useCallback(() => {
    setPendingVoiceText(null)
  }, [])

  // App-level singleton — one set of listeners for the lifetime of the app.
  // Sequential async IIFE + cancelled flag: same pattern as the old useChat fix,
  // but now these listeners live here instead of inside every chat hook instance.
  useEffect(() => {
    let cancelled = false
    const unlisteners: Array<() => void> = []

    ;(async () => {
      const u1 = await listen<boolean>('aria-voice-toggled',     e => setVoiceEnabled(e.payload))
      const u2 = await listen<void>   ('aria-listening-start',   () => setIsListening(true))
      const u3 = await listen<void>   ('aria-listening-stop',    () => setIsListening(false))
      const u4 = await listen<string> ('aria-voice-transcribed', e => setPendingVoiceText(e.payload))
      const u5 = await listen<string> ('aria-voice-error',       e => console.error('[aria] voice error:', e.payload))

      if (cancelled) {
        u1(); u2(); u3(); u4(); u5()
        return
      }
      unlisteners.push(u1, u2, u3, u4, u5)
    })()

    return () => {
      cancelled = true
      unlisteners.forEach(fn => fn())
    }
  }, [])

  return (
    <VoiceContext.Provider value={{ voiceEnabled, isListening, pendingVoiceText, clearPendingVoiceText, toggleVoice }}>
      {children}
    </VoiceContext.Provider>
  )
}
