import { motion } from 'framer-motion'
import type { SVGProps } from 'react'
import type { AriaState } from '../hooks/useAriaState'

// ─── Geometry ────────────────────────────────────────────────────────────────
const CX = 100
const CY = 100
const R_RING = 74       // ring radius = dot-centre radius
const R_EYE = 19        // eye circle radius
const R_PUPIL = 8       // pupil radius
const R_DOT = 7.5       // outer dot radius
const SPOKE_START = R_EYE + 2  // leave a gap after the eye stroke

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

// ─── Transform-origin helper (scale SVG elements around own centre) ───────────
const SO = { transformBox: 'fill-box' as const, transformOrigin: 'center' as const }

// ─── Transition presets ───────────────────────────────────────────────────────
// Pupil + eye: 3.5 s cycle — the heartbeat everything else orbits
const T_CORE = {
  duration: 3.5,
  repeat: Infinity,
  ease: 'easeInOut' as const,
}

// Spokes: slow 5 s cycle, independent from eye
const T_SPOKES = {
  duration: 5,
  repeat: Infinity,
  ease: 'easeInOut' as const,
}

// Ring: same period as spokes but keyframes are inverted → counter-rhythm
const T_RING = {
  duration: 5,
  repeat: Infinity,
  ease: 'easeInOut' as const,
}

// Dots: 4 s wave, each dot staggered 0.15 s clockwise
const tDot = (i: number) => ({
  duration: 4,
  repeat: Infinity,
  ease: 'easeInOut' as const,
  delay: i * 0.15,
})

// Ambient: very slow 6 s breath, tied to core rhythm phase
const T_AMBIENT = {
  duration: 6,
  repeat: Infinity,
  ease: 'easeInOut' as const,
}

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
      {...props}
    >
      <defs>
        {/* Ambient radial gradient – large soft halo behind the whole logo */}
        <radialGradient id="rg-ambient" cx="50%" cy="50%" r="50%">
          <stop offset="0%"   stopColor="#86D5F2" stopOpacity="1" />
          <stop offset="55%"  stopColor="#86D5F2" stopOpacity="0.4" />
          <stop offset="100%" stopColor="#86D5F2" stopOpacity="0" />
        </radialGradient>

        {/*
          f-halo — used on the pupil outer bloom circle.
          Pure blur: the circle itself becomes a diffuse soft cloud.
        */}
        <filter id="f-halo" x="-300%" y="-300%" width="700%" height="700%">
          <feGaussianBlur stdDeviation="7" />
        </filter>

        {/*
          f-core-glow — applied to the pupil core.
          Blurs a copy behind the sharp original → tight luminous halo.
        */}
        <filter id="f-core-glow" x="-150%" y="-150%" width="400%" height="400%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>

        {/*
          f-dot-glow — applied to each outer dot.
          Derives a #86D5F2 bloom from the dot's alpha then merges back,
          giving a brighter halo than the dot's own fill colour.
        */}
        <filter id="f-dot-glow" x="-150%" y="-150%" width="400%" height="400%">
          <feGaussianBlur in="SourceAlpha" stdDeviation="3.5" result="blur" />
          <feFlood floodColor="#86D5F2" floodOpacity="0.65" result="color" />
          <feComposite in="color" in2="blur" operator="in" result="glow" />
          <feMerge>
            <feMergeNode in="glow" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      {/* ── Layer 1: ambient background pulse ── */}
      <motion.circle
        cx={CX} cy={CY} r={88}
        fill="url(#rg-ambient)"
        animate={idle ? { opacity: [0.06, 0.14, 0.06] } : { opacity: 0.05 }}
        transition={T_AMBIENT}
      />

      {/* ── Layer 2: ring — counter-rhythm to spokes (bright when spokes are dim) ── */}
      <motion.circle
        id="ring"
        cx={CX} cy={CY} r={R_RING}
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        animate={idle ? { opacity: [0.7, 0.35, 0.7] } : { opacity: 0.6 }}
        transition={T_RING}
      />

      {/* ── Layer 3: spokes (dim when ring is bright) ── */}
      {ITEMS.map(({ i, sx, sy, dx, dy }) => (
        <motion.line
          key={i}
          id={`spoke-${i}`}
          x1={sx} y1={sy} x2={dx} y2={dy}
          stroke="currentColor"
          strokeWidth={1.5}
          strokeLinecap="round"
          animate={idle ? { opacity: [0.35, 0.72, 0.35] } : { opacity: 0.6 }}
          transition={T_SPOKES}
        />
      ))}

      {/* ── Layer 4: eye — breathes in sync with pupil, slight scale ── */}
      <motion.circle
        id="eye"
        cx={CX} cy={CY} r={R_EYE}
        fill="none"
        stroke="currentColor"
        strokeWidth={2.5}
        style={SO}
        animate={idle ? { opacity: [0.55, 0.92, 0.55], scale: [1, 1.03, 1] } : { opacity: 0.6, scale: 1 }}
        transition={T_CORE}
      />

      {/* ── Layer 5: outer dots — staggered wave traveling clockwise ── */}
      {ITEMS.map(({ i, dx, dy }) => (
        <motion.circle
          key={i}
          id={`dot-${i}`}
          cx={dx} cy={dy} r={R_DOT}
          fill="currentColor"
          filter="url(#f-dot-glow)"
          style={SO}
          animate={idle ? { scale: [1, 1.12, 1] } : { scale: 1 }}
          transition={tDot(i)}
        />
      ))}

      {/* ── Layer 6: pupil outer bloom — large #86D5F2 halo, pulses with core ── */}
      <motion.circle
        cx={CX} cy={CY} r={R_PUPIL * 1.8}
        fill="#86D5F2"
        filter="url(#f-halo)"
        style={SO}
        animate={idle ? { opacity: [0.18, 0.58, 0.18], scale: [1, 1.38, 1] } : { opacity: 0.15, scale: 1 }}
        transition={T_CORE}
      />

      {/* ── Layer 7: pupil core — crisp filled circle with tight glow ── */}
      <motion.circle
        id="pupil"
        cx={CX} cy={CY} r={R_PUPIL}
        fill="currentColor"
        filter="url(#f-core-glow)"
        style={SO}
        animate={idle ? { scale: [1, 1.16, 1] } : { scale: 1 }}
        transition={T_CORE}
      />
    </svg>
  )
}
