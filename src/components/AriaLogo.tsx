import { motion, animate, useMotionValue, useTransform } from 'framer-motion'
import type { MotionValue } from 'framer-motion'
import { useEffect } from 'react'
import type { SVGProps } from 'react'
import type { AriaState } from '../hooks/useAriaState'
import { useThinkingAnimations } from '../hooks/useThinkingAnimations'

// ─── Geometry ────────────────────────────────────────────────────────────────
const CX = 100
const CY = 100
const R_RING = 74
const R_EYE = 19
const R_PUPIL = 8
const R_DOT = 7.5
const R_DOT_HALO = 17
const R_PUPIL_HALO = 14.4
const SPOKE_START = R_EYE + 2
const RING_CIRC = 2 * Math.PI * R_RING        // ≈ 464.9

// Sound waves (speaking)
const WAVE_R = 85
const WAVE_R_START = R_PUPIL / WAVE_R          // ≈ 0.094 — starts at pupil radius
const WAVE_DELAYS = [0, 0.5, 1.0] as const

// Spoke pulse (speaking): 12 px bright + 41 px gap = 53 px ≈ spoke length
const SPOKE_PX = 12
const SPOKE_DASH = `${SPOKE_PX} ${R_RING - SPOKE_START - SPOKE_PX}` // "12 41"
const SPOKE_DASH_END = -(R_RING - SPOKE_START - SPOKE_PX)            // -41

function polar(r: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180
  return [CX + r * Math.cos(rad), CY + r * Math.sin(rad)]
}

const ITEMS = Array.from({ length: 8 }, (_, i) => {
  const deg = i * 45
  const [sx, sy] = polar(SPOKE_START, deg)
  const [dx, dy] = polar(R_RING, deg)
  return { i, sx, sy, dx, dy }
})

const SO = { transformBox: 'fill-box' as const, transformOrigin: 'center' as const }

// ─── Colors ───────────────────────────────────────────────────────────────────
const C_BASE = '#3A8AAA'
const C_PEAK = '#86D5F2'

// ─── Transition presets ───────────────────────────────────────────────────────
const T_CORE             = { duration: 3.5, repeat: Infinity, ease: 'easeInOut' as const }
const T_CORE_FAST        = { duration: 1.5, repeat: Infinity, ease: 'easeInOut' as const }
const T_VOICE            = { duration: 0.6, repeat: Infinity, ease: 'easeInOut' as const }
const T_SPOKES           = { duration: 5,   repeat: Infinity, ease: 'easeInOut' as const }
const T_RING             = { duration: 5,   repeat: Infinity, ease: 'easeInOut' as const }
const T_AMBIENT          = { duration: 6,   repeat: Infinity, ease: 'easeInOut' as const }
const T_AMBIENT_FAST     = { duration: 4,   repeat: Infinity, ease: 'easeInOut' as const }
const T_AMBIENT_DEEP     = { duration: 8,   repeat: Infinity, ease: 'easeInOut' as const }
const T_AMBIENT_DEEP_FAST = { duration: 5,  repeat: Infinity, ease: 'easeInOut' as const }
const T_ATMOSPHERE       = { duration: 7,   repeat: Infinity, ease: 'easeInOut' as const }
const tDot = (i: number) => ({
  duration: 4, repeat: Infinity, ease: 'easeInOut' as const, delay: i * 0.15,
})
const T_SNAP      = { duration: 0.15, ease: 'easeOut' as const }
const T_SNAP_SLOW = { duration: 0.25, ease: 'easeIn'  as const }

// ─── Comet position hook ──────────────────────────────────────────────────────
function usePolar(mv: MotionValue<number>, r: number, offset: number) {
  const x = useTransform(mv, a => CX + r * Math.cos(((a + offset) * Math.PI) / 180))
  const y = useTransform(mv, a => CY + r * Math.sin(((a + offset) * Math.PI) / 180))
  return { x, y }
}

// ─── Component ───────────────────────────────────────────────────────────────
interface Props extends SVGProps<SVGSVGElement> {
  state?: AriaState
}

export default function AriaLogo({ state = 'idle', ...props }: Props) {
  const thinking = state === 'thinking'
  const speaking = state === 'speaking'

  const { dotFlash, signalingSpoke, synapticDots } = useThinkingAnimations(state)

  // ── Comet (thinking only) ─────────────────────────────────────────────────
  const cometAngle = useMotionValue(0)
  const cometHead  = usePolar(cometAngle, R_RING, 0)
  const cometTail1 = usePolar(cometAngle, R_RING, -14)
  const cometTail2 = usePolar(cometAngle, R_RING, -28)
  const cometTail3 = usePolar(cometAngle, R_RING, -42)

  useEffect(() => {
    if (!thinking) { cometAngle.set(0); return }
    const controls = animate(cometAngle, [0, 360], {
      duration: 3, repeat: Infinity, ease: 'linear', repeatType: 'loop',
    })
    return () => controls.stop()
  }, [thinking, cometAngle])

  // ── Synaptic line positions ───────────────────────────────────────────────
  const synapticPos = synapticDots ? (() => {
    const [x1, y1] = polar(R_RING, synapticDots[0] * 45)
    const [x2, y2] = polar(R_RING, synapticDots[1] * 45)
    return { x1, y1, x2, y2 }
  })() : null

  return (
    <svg
      viewBox="0 0 200 200"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Aria"
      role="img"
      overflow="visible"
      {...props}
    >
      <defs>
        <radialGradient id="rg-ambient" cx="50%" cy="50%" r="50%">
          <stop offset="0%"   stopColor={C_PEAK} stopOpacity="1" />
          <stop offset="50%"  stopColor={C_PEAK} stopOpacity="0.5" />
          <stop offset="100%" stopColor={C_PEAK} stopOpacity="0" />
        </radialGradient>
        <radialGradient id="rg-ambient-deep" cx="50%" cy="50%" r="50%">
          <stop offset="0%"   stopColor={C_PEAK} stopOpacity="0.5" />
          <stop offset="65%"  stopColor={C_PEAK} stopOpacity="0.15" />
          <stop offset="100%" stopColor={C_PEAK} stopOpacity="0" />
        </radialGradient>

        <filter id="f-halo" x="-400%" y="-400%" width="900%" height="900%">
          <feGaussianBlur stdDeviation="14" />
        </filter>
        <filter id="f-core-glow" x="-200%" y="-200%" width="500%" height="500%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="3.5" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
        <filter id="f-dot-halo" x="-250%" y="-250%" width="600%" height="600%">
          <feGaussianBlur stdDeviation="8" />
        </filter>
        <filter id="f-ring-glow" filterUnits="userSpaceOnUse" x="-15" y="-15" width="230" height="230">
          <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
        <filter id="f-spoke-glow" filterUnits="userSpaceOnUse" x="-15" y="-15" width="230" height="230">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.5" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
        <filter id="f-atmosphere" filterUnits="userSpaceOnUse" x="-100" y="-100" width="400" height="400">
          <feGaussianBlur stdDeviation="20" />
        </filter>
        <filter id="f-comet" x="-300%" y="-300%" width="700%" height="700%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="3" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
        {/* Soft edge blur on expanding sound waves */}
        <filter id="f-wave" filterUnits="userSpaceOnUse" x="-60" y="-60" width="320" height="320">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.2" />
        </filter>
      </defs>

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 1 — Deep ambient
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={130}
        fill="url(#rg-ambient-deep)"
        animate={
          thinking  ? { opacity: [0.10, 0.24, 0.10] }
          : speaking ? { opacity: [0.12, 0.28, 0.12] }
          :             { opacity: [0.07, 0.16, 0.07] }
        }
        transition={
          thinking  ? T_AMBIENT_DEEP_FAST
          : speaking ? T_VOICE
          :             T_AMBIENT_DEEP
        }
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 2 — Mid ambient
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={90}
        fill="url(#rg-ambient)"
        animate={
          thinking  ? { opacity: [0.26, 0.52, 0.26] }
          : speaking ? { opacity: [0.22, 0.44, 0.22] }
          :             { opacity: [0.18, 0.34, 0.18] }
        }
        transition={
          thinking  ? T_AMBIENT_FAST
          : speaking ? T_VOICE
          :             T_AMBIENT
        }
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 3 — Atmosphere ring
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={95}
        fill="none" stroke={C_PEAK} strokeWidth={22}
        filter="url(#f-atmosphere)"
        animate={
          thinking  ? { opacity: [0.07, 0.18, 0.07] }
          : speaking ? { opacity: [0.10, 0.22, 0.10] }
          :             { opacity: [0.05, 0.15, 0.05] }
        }
        transition={speaking ? T_VOICE : T_ATMOSPHERE}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 4 — Ring
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        id="ring"
        cx={CX} cy={CY} r={R_RING}
        fill="none" strokeWidth={2}
        filter="url(#f-ring-glow)"
        strokeDasharray={thinking ? '460 5' : undefined}
        animate={
          thinking
            ? { strokeDashoffset: [0, -RING_CIRC], stroke: C_PEAK, opacity: 0.85 }
          : speaking
            ? { strokeDashoffset: 0, stroke: C_PEAK, opacity: [0.5, 1.0, 0.5] }
          :   { strokeDashoffset: 0, stroke: [C_PEAK, C_BASE, C_PEAK], opacity: [0.75, 0.35, 0.75] }
        }
        transition={
          thinking
            ? { strokeDashoffset: { duration: 12, repeat: Infinity, ease: 'linear' }, default: { duration: 0.5 } }
          : speaking ? T_VOICE
          : T_RING
        }
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 5 — Spokes (base layer, all states)
      ═══════════════════════════════════════════════════════════════════════ */}
      {ITEMS.map(({ i, sx, sy, dx, dy }) => (
        <motion.line
          key={i}
          id={`spoke-${i}`}
          x1={sx} y1={sy} x2={dx} y2={dy}
          stroke={C_BASE} strokeWidth={1.5} strokeLinecap="round"
          filter="url(#f-spoke-glow)"
          animate={
            thinking
              ? { stroke: signalingSpoke === i ? C_PEAK : C_BASE,
                  opacity: signalingSpoke === i ? 0.92 : 0.30 }
            : speaking
              ? { stroke: C_PEAK, opacity: 0.65 }
            :   { stroke: [C_BASE, C_PEAK, C_BASE], opacity: [0.38, 0.85, 0.38] }
          }
          transition={thinking ? T_SNAP : speaking ? { duration: 0.3 } : T_SPOKES}
        />
      ))}

      {/* ── Spoke pulse overlays — outward energy flow (speaking only) ────── */}
      {speaking && ITEMS.map(({ i, sx, sy, dx, dy }) => (
        <motion.line
          key={`sp-${i}`}
          x1={sx} y1={sy} x2={dx} y2={dy}
          stroke={C_PEAK} strokeWidth={2} strokeLinecap="round"
          strokeDasharray={SPOKE_DASH}
          filter="url(#f-spoke-glow)"
          animate={{ strokeDashoffset: [0, SPOKE_DASH_END] }}
          transition={{ duration: 0.5, repeat: Infinity, ease: 'easeIn' }}
        />
      ))}

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 6 — Sound waves — expand from pupil outward (speaking only)
          vectorEffect keeps the stroke thin regardless of CSS scale
      ═══════════════════════════════════════════════════════════════════════ */}
      {WAVE_DELAYS.map((delay, i) => (
        <motion.circle
          key={`wave-${i}`}
          cx={CX} cy={CY} r={WAVE_R}
          fill="none" stroke={C_PEAK} strokeWidth={1.2}
          vectorEffect="non-scaling-stroke"
          filter="url(#f-wave)"
          style={SO}
          animate={
            speaking
              ? { scale: [WAVE_R_START, 1.0], opacity: [0.55, 0] }
              : { scale: 0, opacity: 0 }
          }
          transition={
            speaking
              ? { duration: 1.5, repeat: Infinity, ease: 'easeOut', delay }
              : { duration: 0.4 }
          }
        />
      ))}

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 7 — Eye
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        id="eye"
        cx={CX} cy={CY} r={R_EYE}
        fill="none" stroke={C_BASE} strokeWidth={2.5}
        filter="url(#f-ring-glow)"
        style={SO}
        animate={
          thinking
            ? { stroke: C_PEAK, opacity: [0.65, 1.0, 0.65], scale: [1, 1.04, 1] }
          : speaking
            ? { stroke: C_PEAK, opacity: [0.60, 1.0, 0.60], scale: [1, 1.04, 1] }
          :   { stroke: [C_BASE, C_PEAK, C_BASE], opacity: [0.55, 0.96, 0.55], scale: [1, 1.03, 1] }
        }
        transition={thinking ? T_CORE_FAST : speaking ? T_VOICE : T_CORE}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 8 — Outer dots
      ═══════════════════════════════════════════════════════════════════════ */}
      {ITEMS.map(({ i, dx, dy }) => {
        const flashing = thinking && dotFlash[i]
        return (
          <motion.g
            key={i}
            style={SO}
            animate={
              thinking  ? { scale: flashing ? 1.4 : 1 }
              : speaking ? { scale: [1, 1.15, 1] }
              :             { scale: [1, 1.13, 1] }
            }
            transition={
              thinking  ? (flashing ? T_SNAP : T_SNAP_SLOW)
              : speaking ? T_VOICE
              :             tDot(i)
            }
          >
            <motion.circle
              cx={dx} cy={dy} r={R_DOT_HALO}
              fill={C_PEAK} filter="url(#f-dot-halo)"
              animate={
                thinking  ? { opacity: flashing ? 0.95 : 0.52 }
                : speaking ? { opacity: [0.30, 0.92, 0.30] }
                :             { opacity: [0.25, 0.50, 0.25] }
              }
              transition={
                thinking  ? T_SNAP
                : speaking ? T_VOICE
                :             tDot(i)
              }
            />
            <motion.circle
              id={`dot-${i}`}
              cx={dx} cy={dy} r={R_DOT}
              fill={C_BASE}
              animate={
                thinking  ? { fill: flashing ? C_PEAK : C_BASE, opacity: 1 }
                : speaking ? { fill: [C_BASE, C_PEAK, C_BASE], opacity: 1 }
                :             { fill: C_BASE, opacity: 0.75 }
              }
              transition={
                thinking  ? T_SNAP
                : speaking ? T_VOICE
                :             tDot(i)
              }
            />
          </motion.g>
        )
      })}

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 9 — Pupil outer bloom
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={R_PUPIL_HALO}
        fill={C_PEAK} filter="url(#f-halo)"
        style={SO}
        animate={
          thinking  ? { opacity: [0.28, 0.92, 0.28], scale: [1, 1.9,  1] }
          : speaking ? { opacity: [0.30, 1.00, 0.30], scale: [1, 2.2,  1] }
          :             { opacity: [0.22, 0.88, 0.22], scale: [1, 1.8,  1] }
        }
        transition={thinking ? T_CORE_FAST : speaking ? T_VOICE : T_CORE}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 10 — Pupil core  (the voice source)
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        id="pupil"
        cx={CX} cy={CY} r={R_PUPIL}
        fill={C_BASE} filter="url(#f-core-glow)"
        style={SO}
        animate={
          thinking  ? { fill: [C_BASE, C_PEAK, C_BASE], scale: [1, 1.22, 1] }
          : speaking ? { fill: [C_BASE, C_PEAK, C_BASE], scale: [1, 1.25, 1] }
          :             { fill: [C_BASE, C_PEAK, C_BASE], scale: [1, 1.16, 1] }
        }
        transition={thinking ? T_CORE_FAST : speaking ? T_VOICE : T_CORE}
      />

      {/* ── Comet (thinking only) ─────────────────────────────────────────── */}
      {thinking && (
        <>
          <motion.circle r={2}   fill={C_PEAK} opacity={0.15} style={{ x: cometTail3.x, y: cometTail3.y }} />
          <motion.circle r={2.5} fill={C_PEAK} opacity={0.32} style={{ x: cometTail2.x, y: cometTail2.y }} />
          <motion.circle r={3}   fill={C_PEAK} opacity={0.58} style={{ x: cometTail1.x, y: cometTail1.y }} />
          <motion.circle r={4}   fill={C_PEAK} filter="url(#f-comet)" style={{ x: cometHead.x, y: cometHead.y }} />
        </>
      )}

      {/* ── Synaptic connection (thinking only) ───────────────────────────── */}
      {thinking && synapticPos && (
        <motion.line
          key={`syn-${synapticDots![0]}-${synapticDots![1]}`}
          x1={synapticPos.x1} y1={synapticPos.y1}
          x2={synapticPos.x2} y2={synapticPos.y2}
          stroke={C_PEAK} strokeWidth={0.8}
          filter="url(#f-spoke-glow)"
          animate={{ opacity: [0, 0.5, 0] }}
          transition={{ duration: 0.6, ease: 'easeInOut' }}
        />
      )}
    </svg>
  )
}
