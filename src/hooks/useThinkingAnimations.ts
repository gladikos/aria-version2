import { useState, useEffect } from 'react'
import type { AriaState } from './useAriaState'

const CX = 100
const CY = 100

function mkRng(seed: number) {
  let s = seed >>> 0
  return () => {
    s = (Math.imul(1664525, s) + 1013904223) >>> 0
    return s / 0xffffffff
  }
}

const SESSION_SEED = Math.floor(Math.random() * 100000)

function buildAutoencoderPositions(): [number, number][] {
  const rng    = mkRng(SESSION_SEED + 77)
  const cyBase = [45, 82, 118, 155]
  const pos: [number, number][] = []

  // Dots 0-3 → left column (input layer)
  for (let i = 0; i < 4; i++) {
    pos.push([
      45  + (rng() - 0.5) * 6,
      cyBase[i] + (rng() - 0.5) * 6,
    ])
  }
  // Dots 4-7 → right column (output layer)
  for (let i = 0; i < 4; i++) {
    pos.push([
      155 + (rng() - 0.5) * 6,
      cyBase[i] + (rng() - 0.5) * 6,
    ])
  }
  return pos
}

export const THINKING_POSITIONS: readonly [number, number][] = buildAutoencoderPositions()

// Decorative texture only — never carry signals
export const LEFT_CROSS_PAIRS:  readonly [number, number][] = [[0,1],[1,2],[0,3]]
export const RIGHT_CROSS_PAIRS: readonly [number, number][] = [[4,5],[5,6],[4,7]]

export interface ForwardSignal {
  id:     number
  dotIdx: number
  x1: number; y1: number
  x2: number; y2: number
}

let nextId = 0

function pickN(arr: number[], n: number): number[] {
  return [...arr].sort(() => Math.random() - 0.5).slice(0, n)
}

export function useThinkingAnimations(state: AriaState) {
  const [inputSignals,  setInputSignals]  = useState<ForwardSignal[]>([])
  const [outputSignals, setOutputSignals] = useState<ForwardSignal[]>([])
  const [flashingDots,  setFlashingDots]  = useState<number[]>([])
  const [pupilBoost,    setPupilBoost]    = useState<'none' | 'standard' | 'deep'>('none')
  const [pupilBoostId,  setPupilBoostId]  = useState(0)
  const [idleFiringDot, setIdleFiringDot] = useState<number | null>(null)

  const thinking = state === 'thinking'
  const idle     = state === 'idle'

  // ── Forward pass scheduler ────────────────────────────────────────────────
  useEffect(() => {
    if (!thinking) {
      setInputSignals([])
      setOutputSignals([])
      setFlashingDots([])
      setPupilBoost('none')
      return
    }

    let alive = true
    const timers: ReturnType<typeof setTimeout>[] = []
    const nextDeep = { v: Date.now() + 8000 + Math.random() * 7000 }

    function runPass() {
      if (!alive) return

      const isDeep   = Date.now() >= nextDeep.v
      if (isDeep) nextDeep.v = Date.now() + 8000 + Math.random() * 7000

      const leftDots = isDeep ? [0,1,2,3] : pickN([0,1,2,3], 1 + Math.floor(Math.random() * 3))

      // Phase 1 — input orbs: left dots → pupil
      setInputSignals(leftDots.map(dotIdx => ({
        id: ++nextId, dotIdx,
        x1: THINKING_POSITIONS[dotIdx][0],
        y1: THINKING_POSITIONS[dotIdx][1],
        x2: CX, y2: CY,
      })))

      // Phase 2 — orbs arrive at pupil after 500ms travel
      const t1 = setTimeout(() => {
        if (!alive) return
        setInputSignals([])
        setPupilBoost(isDeep ? 'deep' : 'standard')
        setPupilBoostId(id => id + 1)

        const flashMs = isDeep ? 500 : 300

        // Phase 3 — pupil done, fire output
        const t2 = setTimeout(() => {
          if (!alive) return
          setPupilBoost('none')

          const rightDots = isDeep ? [4,5,6,7] : pickN([4,5,6,7], 1 + Math.floor(Math.random() * 3))

          setOutputSignals(rightDots.map(dotIdx => ({
            id: ++nextId, dotIdx,
            x1: CX, y1: CY,
            x2: THINKING_POSITIONS[dotIdx][0],
            y2: THINKING_POSITIONS[dotIdx][1],
          })))

          // Phase 4 — output orbs arrive after 500ms
          const t3 = setTimeout(() => {
            if (!alive) return
            setOutputSignals([])
            setFlashingDots(rightDots)

            const t4 = setTimeout(() => {
              if (!alive) return
              setFlashingDots([])

              const t5 = setTimeout(() => { if (alive) runPass() }, 2000 + Math.random() * 1000)
              timers.push(t5)
            }, 400)
            timers.push(t4)
          }, 500)
          timers.push(t3)
        }, flashMs)
        timers.push(t2)
      }, 500)
      timers.push(t1)
    }

    const t0 = setTimeout(runPass, 600)
    timers.push(t0)
    return () => { alive = false; timers.forEach(clearTimeout) }
  }, [thinking])

  // ── Idle dot color-pop ───────────────────────────────────────────────────
  useEffect(() => {
    if (!idle) { setIdleFiringDot(null); return }

    let alive = true
    let timer: ReturnType<typeof setTimeout>

    const schedule = () => {
      timer = setTimeout(() => {
        if (!alive) return
        setIdleFiringDot(Math.floor(Math.random() * 8))
        setTimeout(() => { if (alive) setIdleFiringDot(null) }, 700)
        schedule()
      }, 4000 + Math.random() * 3000)
    }

    schedule()
    return () => { alive = false; clearTimeout(timer) }
  }, [idle])

  return { inputSignals, outputSignals, flashingDots, pupilBoost, pupilBoostId, idleFiringDot }
}
