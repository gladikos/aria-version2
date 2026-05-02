import { motion } from 'framer-motion'
import type { SVGProps } from 'react'
import type { AriaState } from '../hooks/useAriaState'

// ─── Geometry ────────────────────────────────────────────────────────────────
const CX = 100
const CY = 100
const R_RING = 74
const R_EYE = 19
const R_PUPIL = 8
const R_DOT = 7.5
const R_DOT_HALO = 17       // bloom circle radius — 2.3× dot, heavily blurred
const R_PUPIL_HALO = 14.4   // 1.8× pupil, becomes a star via f-halo blur
const SPOKE_START = R_EYE + 2

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

// ─── Scale/origin helper for SVG elements ────────────────────────────────────
const SO = { transformBox: 'fill-box' as const, transformOrigin: 'center' as const }

// ─── Color palette ───────────────────────────────────────────────────────────
const C_BASE = '#3A8AAA'   // core resting colour
const C_PEAK = '#86D5F2'   // peak-brightness colour (cooler, brighter)

// ─── Transition presets ───────────────────────────────────────────────────────
const T_CORE        = { duration: 3.5, repeat: Infinity, ease: 'easeInOut' as const }
const T_SPOKES      = { duration: 5,   repeat: Infinity, ease: 'easeInOut' as const }
const T_RING        = { duration: 5,   repeat: Infinity, ease: 'easeInOut' as const }
const T_AMBIENT     = { duration: 6,   repeat: Infinity, ease: 'easeInOut' as const }
const T_AMBIENT_DEEP = { duration: 8,  repeat: Infinity, ease: 'easeInOut' as const }
const T_ATMOSPHERE  = { duration: 7,   repeat: Infinity, ease: 'easeInOut' as const }
const tDot = (i: number) => ({
  duration: 4,
  repeat: Infinity,
  ease: 'easeInOut' as const,
  delay: i * 0.15,
})

// ─── Component ───────────────────────────────────────────────────────────────
interface Props extends SVGProps<SVGSVGElement> {
  state?: AriaState
}

export default function AriaLogo({ state = 'idle', ...props }: Props) {
  const idle = state === 'idle'

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
        {/* Two ambient gradient scales — same colour, different spread */}
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

        {/*
          f-halo — pure blur applied to the pupil outer bloom circle.
          Turns a small filled circle into a diffuse star-like cloud.
          Large region because the blur spreads well beyond the source.
        */}
        <filter id="f-halo" x="-400%" y="-400%" width="900%" height="900%">
          <feGaussianBlur stdDeviation="14" />
        </filter>

        {/*
          f-core-glow — blur+merge on pupil core.
          Adds a tight luminous fringe without softening the crisp dot.
        */}
        <filter id="f-core-glow" x="-200%" y="-200%" width="500%" height="500%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="3.5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>

        {/*
          f-dot-halo — pure blur applied to the per-dot bloom circle (filled C_PEAK).
          The unblurred dot core is layered on top separately.
        */}
        <filter id="f-dot-halo" x="-250%" y="-250%" width="600%" height="600%">
          <feGaussianBlur stdDeviation="8" />
        </filter>

        {/*
          f-ring-glow — blur+merge giving the ring stroke a luminous edge.
          filterUnits="userSpaceOnUse" avoids percentage-region issues on large paths.
        */}
        <filter id="f-ring-glow" filterUnits="userSpaceOnUse" x="-15" y="-15" width="230" height="230">
          <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>

        {/*
          f-spoke-glow — thinner version of ring glow for the spokes.
        */}
        <filter id="f-spoke-glow" filterUnits="userSpaceOnUse" x="-15" y="-15" width="230" height="230">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>

        {/*
          f-atmosphere — extreme blur for the outer atmosphere ring.
          Spreads ~54 px from the stroke edge; region extends well beyond SVG bounds.
        */}
        <filter id="f-atmosphere" filterUnits="userSpaceOnUse" x="-100" y="-100" width="400" height="400">
          <feGaussianBlur stdDeviation="20" />
        </filter>
      </defs>

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 1 — Deep ambient (largest, slowest, most diffuse)
          r=130 bleeds well past the 400 px rendered logo via overflow="visible"
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={130}
        fill="url(#rg-ambient-deep)"
        animate={idle ? { opacity: [0.07, 0.16, 0.07] } : { opacity: 0.06 }}
        transition={T_AMBIENT_DEEP}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 2 — Mid ambient (tighter, brighter, 6 s)
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={90}
        fill="url(#rg-ambient)"
        animate={idle ? { opacity: [0.18, 0.34, 0.18] } : { opacity: 0.14 }}
        transition={T_AMBIENT}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 3 — Atmosphere ring  (outer halo of light surrounding the piece)
          Stroked circle at r=95, heavily blurred → a soft band of light
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={95}
        fill="none"
        stroke={C_PEAK}
        strokeWidth={22}
        filter="url(#f-atmosphere)"
        animate={idle ? { opacity: [0.05, 0.15, 0.05] } : { opacity: 0.05 }}
        transition={T_ATMOSPHERE}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 4 — Ring  (counter-phase to spokes; bright = C_PEAK, dim = C_BASE)
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        id="ring"
        cx={CX} cy={CY} r={R_RING}
        fill="none"
        stroke={C_PEAK}
        strokeWidth={2}
        filter="url(#f-ring-glow)"
        animate={idle
          ? { stroke: [C_PEAK, C_BASE, C_PEAK], opacity: [0.75, 0.35, 0.75] }
          : { stroke: C_BASE, opacity: 0.6 }
        }
        transition={T_RING}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 5 — Spokes  (dim when ring is bright; shift to C_PEAK at peak)
      ═══════════════════════════════════════════════════════════════════════ */}
      {ITEMS.map(({ i, sx, sy, dx, dy }) => (
        <motion.line
          key={i}
          id={`spoke-${i}`}
          x1={sx} y1={sy} x2={dx} y2={dy}
          stroke={C_BASE}
          strokeWidth={1.5}
          strokeLinecap="round"
          filter="url(#f-spoke-glow)"
          animate={idle
            ? { stroke: [C_BASE, C_PEAK, C_BASE], opacity: [0.38, 0.85, 0.38] }
            : { stroke: C_BASE, opacity: 0.6 }
          }
          transition={T_SPOKES}
        />
      ))}

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 6 — Eye  (breathes with pupil; slight scale + colour shift)
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        id="eye"
        cx={CX} cy={CY} r={R_EYE}
        fill="none"
        stroke={C_BASE}
        strokeWidth={2.5}
        filter="url(#f-ring-glow)"
        style={SO}
        animate={idle
          ? { stroke: [C_BASE, C_PEAK, C_BASE], opacity: [0.55, 0.96, 0.55], scale: [1, 1.03, 1] }
          : { stroke: C_BASE, opacity: 0.6, scale: 1 }
        }
        transition={T_CORE}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 7 — Outer dots  (each dot = a distinct light source)
          motion.g drives the scale; inner circles handle colour + halo opacity
      ═══════════════════════════════════════════════════════════════════════ */}
      {ITEMS.map(({ i, dx, dy }) => (
        <motion.g
          key={i}
          style={SO}
          animate={idle ? { scale: [1, 1.13, 1] } : { scale: 1 }}
          transition={tDot(i)}
        >
          {/* Bloom halo — C_PEAK blurred cloud, baseline opacity never drops to zero */}
          <motion.circle
            cx={dx} cy={dy} r={R_DOT_HALO}
            fill={C_PEAK}
            filter="url(#f-dot-halo)"
            animate={idle ? { opacity: [0.52, 0.92, 0.52] } : { opacity: 0.45 }}
            transition={tDot(i)}
          />
          {/* Dot core — colour pulses from base to peak */}
          <motion.circle
            id={`dot-${i}`}
            cx={dx} cy={dy} r={R_DOT}
            fill={C_BASE}
            animate={idle
              ? { fill: [C_BASE, C_PEAK, C_BASE] }
              : { fill: C_BASE }
            }
            transition={tDot(i)}
          />
        </motion.g>
      ))}

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 8 — Pupil outer bloom  (the "sun" — large, intense, fast)
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        cx={CX} cy={CY} r={R_PUPIL_HALO}
        fill={C_PEAK}
        filter="url(#f-halo)"
        style={SO}
        animate={idle
          ? { opacity: [0.22, 0.88, 0.22], scale: [1, 1.8, 1] }
          : { opacity: 0.18, scale: 1 }
        }
        transition={T_CORE}
      />

      {/* ══════════════════════════════════════════════════════════════════════
          LAYER 9 — Pupil core  (crisp dot with tight glow + colour shift)
      ═══════════════════════════════════════════════════════════════════════ */}
      <motion.circle
        id="pupil"
        cx={CX} cy={CY} r={R_PUPIL}
        fill={C_BASE}
        filter="url(#f-core-glow)"
        style={SO}
        animate={idle
          ? { fill: [C_BASE, C_PEAK, C_BASE], scale: [1, 1.16, 1] }
          : { fill: C_BASE, scale: 1 }
        }
        transition={T_CORE}
      />
    </svg>
  )
}
