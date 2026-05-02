import { useState } from 'react'

export type AriaState = 'idle' | 'thinking' | 'speaking'

export function useAriaState() {
  const [state, setState] = useState<AriaState>('idle')
  return { state, setState }
}
