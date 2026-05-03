import { useState, useEffect } from 'react'
import type { AriaState } from './useAriaState'

export function useThinkingAnimations(state: AriaState) {
  const thinking = state === 'thinking'

  const [dotFlash, setDotFlash] = useState<boolean[]>(Array(8).fill(false))
  const [signalingSpoke, setSignalingSpoke] = useState<number | null>(null)
  const [synapticDots, setSynapticDots] = useState<[number, number] | null>(null)

  // Random dot flashes — each dot independently fires at random intervals 0.4–1.5s
  useEffect(() => {
    if (!thinking) { setDotFlash(Array(8).fill(false)); return }
    let active = true
    const timers = new Set<ReturnType<typeof setTimeout>>()

    function scheduleFlash(i: number) {
      const t = setTimeout(() => {
        if (!active) return
        timers.delete(t)
        setDotFlash(prev => { const n = [...prev]; n[i] = true; return n })
        const t2 = setTimeout(() => {
          if (!active) return
          timers.delete(t2)
          setDotFlash(prev => { const n = [...prev]; n[i] = false; return n })
          if (active) scheduleFlash(i)
        }, 150 + Math.random() * 100)
        timers.add(t2)
      }, 400 + Math.random() * 1100)
      timers.add(t)
    }

    for (let i = 0; i < 8; i++) scheduleFlash(i)
    return () => { active = false; timers.forEach(clearTimeout); setDotFlash(Array(8).fill(false)) }
  }, [thinking])

  // Spoke signals — one random spoke brightens every 0.8–2s
  useEffect(() => {
    if (!thinking) { setSignalingSpoke(null); return }
    let active = true
    const timers: ReturnType<typeof setTimeout>[] = []

    function schedule() {
      const t1 = setTimeout(() => {
        if (!active) return
        setSignalingSpoke(Math.floor(Math.random() * 8))
        const t2 = setTimeout(() => {
          if (!active) return
          setSignalingSpoke(null)
          schedule()
        }, 300 + Math.random() * 200)
        timers.push(t2)
      }, 800 + Math.random() * 1200)
      timers.push(t1)
    }

    schedule()
    return () => { active = false; timers.forEach(clearTimeout); setSignalingSpoke(null) }
  }, [thinking])

  // Synaptic connections — faint line between two non-adjacent dots, every 2.5–5s
  useEffect(() => {
    if (!thinking) { setSynapticDots(null); return }
    let active = true
    const timers: ReturnType<typeof setTimeout>[] = []

    function schedule() {
      const t1 = setTimeout(() => {
        if (!active) return
        const i = Math.floor(Math.random() * 8)
        const j = (i + 2 + Math.floor(Math.random() * 5)) % 8
        setSynapticDots([i, j])
        const t2 = setTimeout(() => {
          if (!active) return
          setSynapticDots(null)
          schedule()
        }, 600)
        timers.push(t2)
      }, 2500 + Math.random() * 2500)
      timers.push(t1)
    }

    schedule()
    return () => { active = false; timers.forEach(clearTimeout); setSynapticDots(null) }
  }, [thinking])

  return { dotFlash, signalingSpoke, synapticDots }
}
