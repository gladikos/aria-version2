import { motion, AnimatePresence, animate, useMotionValue } from 'framer-motion'
import { useState, useEffect, useRef } from 'react'
import type { SVGProps } from 'react'
import type { AriaState } from '../hooks/useAriaState'
import {
  useThinkingAnimations,
  THINKING_POSITIONS,
  LEFT_CROSS_PAIRS,
  RIGHT_CROSS_PAIRS,
  type ForwardSignal,
} from '../hooks/useThinkingAnimations'

// ─── Geometry ────────────────────────────────────────────────────────────────
const CX           = 100
const CY           = 100
const R_RING       = 74
const R_EYE        = 19
const R_PUPIL      = 8
const R_DOT        = 7.5
const R_PUPIL_HALO = 14.4
const SPOKE_START  = R_EYE + 2
const WAVE_R       = 85
const WAVE_R_START = R_PUPIL / WAVE_R
const WAVE_DELAYS  = [0, 0.5, 1.0] as const
const SPOKE_PX     = 12
const SPOKE_DASH   = `${SPOKE_PX} ${R_RING - SPOKE_START - SPOKE_PX}`
const SPOKE_DASH_END = -(R_RING - SPOKE_START - SPOKE_PX)
const SCATTER_DIST = 18

function polar(r: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180
  return [CX + r * Math.cos(rad), CY + r * Math.sin(rad)]
}

const ITEMS = Array.from({ length: 8 }, (_, i) => {
  const deg      = i * 45
  const [sx, sy] = polar(SPOKE_START, deg)
  const [dx, dy] = polar(R_RING, deg)
  const scatterX = ((dx - CX) / R_RING) * SCATTER_DIST
  const scatterY = ((dy - CY) / R_RING) * SCATTER_DIST
  return { i, sx, sy, dx, dy, scatterX, scatterY }
})

const SO = { transformBox: 'fill-box' as const, transformOrigin: 'center' as const }

const C_BASE = '#3A8AAA'
const C_PEAK = '#86D5F2'

// ─── Transition presets ───────────────────────────────────────────────────────
const T_CORE   = { duration: 3.5, repeat: Infinity, ease: 'easeInOut' as const }
const T_VOICE  = { duration: 0.6, repeat: Infinity, ease: 'easeInOut' as const }
const T_SPOKES = { duration: 5,   repeat: Infinity, ease: 'easeInOut' as const }
const T_RING   = { duration: 5,   repeat: Infinity, ease: 'easeInOut' as const }
const T_BRAND  = { duration: 0.6, ease: 'easeOut' as const }
const tDot = (i: number) => ({ duration: 4, repeat: Infinity, ease: 'easeInOut' as const, delay: i * 0.15 })

type MorphPhase = 'idle' | 'deconstructing' | 'reconstructing'

// ─── Signal orb ──────────────────────────────────────────────────────────────
function SignalOrb({ x1, y1, x2, y2 }: { x1: number; y1: number; x2: number; y2: number }) {
  const x = useMotionValue(x1)
  const y = useMotionValue(y1)

  useEffect(() => {
    const cx = animate(x, x2, { duration: 0.5, ease: 'easeInOut' })
    const cy = animate(y, y2, { duration: 0.5, ease: 'easeInOut' })
    return () => { cx.stop(); cy.stop() }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <motion.circle
      r={3} fill={C_PEAK} filter="url(#f-orb)"
      style={{ x, y }}
      initial={{ opacity: 0.9 }}
      animate={{ opacity: 0.9 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.3 }}
    />
  )
}

// ─── Props ────────────────────────────────────────────────────────────────────
interface Props extends SVGProps<SVGSVGElement> {
  state?: AriaState
  mode?:  'brand' | 'animated'
}

export default function AriaLogo({ state = 'idle', mode, ...props }: Props) {

  const [effectiveMode, setEffectiveMode] = useState<'brand' | 'animated'>(
    mode === 'brand' ? 'brand' : 'animated'
  )
  const [morphPhase, setMorphPhase] = useState<MorphPhase>('idle')
  const [targetMode, setTargetMode] = useState<'brand' | 'animated'>(
    mode === 'brand' ? 'brand' : 'animated'
  )
  const prevModeRef = useRef<'brand' | 'animated' | undefined>(undefined)

  // Detect mode changes and run the 3-phase deconstruct/travel/reconstruct sequence.
  // Total: 1.6s — deconstruct 0-0.5s, travel 0-1.0s (via container), reconstruct 1.1-1.6s.
  useEffect(() => {
    const newTarget = mode === 'brand' ? 'brand' : 'animated'

    if (prevModeRef.current === undefined) {
      prevModeRef.current = newTarget
      return
    }
    if (prevModeRef.current === newTarget) return
    prevModeRef.current = newTarget

    setTargetMode(newTarget)
    setMorphPhase('deconstructing')

    const t1 = setTimeout(() => setMorphPhase('reconstructing'), 1100)
    const t2 = setTimeout(() => {
      setMorphPhase('idle')
      setEffectiveMode(newTarget)
    }, 1600)

    return () => { clearTimeout(t1); clearTimeout(t2) }
  }, [mode])

  const morph        = morphPhase !== 'idle'
  const deconstructing = morphPhase === 'deconstructing'
  const reconstructing = morphPhase === 'reconstructing'

  const brand    = !morph && effectiveMode === 'brand'
  const thinking = !morph && !brand && state === 'thinking'
  const speaking = !morph && !brand && state === 'speaking'

  const {
    inputSignals, outputSignals,
    flashingDots, pupilBoost, pupilBoostId,
    idleFiringDot,
  } = useThinkingAnimations(state)

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
        <filter id="f-atmosphere" filterUnits="userSpaceOnUse" x="-100" y="-100" width="400" height="400">
          <feGaussianBlur stdDeviation="20" />
        </filter>
        <filter id="f-wave" filterUnits="userSpaceOnUse" x="-60" y="-60" width="320" height="320">
          <feGaussianBlur in="SourceGraphic" stdDeviation="1.2" />
        </filter>
        <filter id="f-bloom" x="-150%" y="-150%" width="400%" height="400%">
          <feGaussianBlur stdDeviation="18" />
        </filter>
        <filter id="f-orb" x="-300%" y="-300%" width="700%" height="700%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="2" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
      </defs>

      {/* ── State-change bloom flash — suppressed during morph and brand mode ── */}
      <motion.circle
        key={morph ? 'morph-static' : brand ? 'brand-static' : state}
        cx={CX} cy={CY} r={85}
        fill="url(#rg-ambient)" filter="url(#f-bloom)"
        style={SO}
        initial={{ opacity: (morph || brand) ? 0 : 0.55, scale: 1.3 }}
        animate={{ opacity: 0, scale: 1 }}
        transition={{ duration: 0.65, ease: 'easeOut' }}
      />

      {/* ── Deep ambient ─────────────────────────────────────────────────────── */}
      <motion.circle
        cx={CX} cy={CY} r={130}
        fill="url(#rg-ambient-deep)"
        animate={{
          opacity: deconstructing ? 0
            : reconstructing      ? (targetMode === 'brand' ? 0 : 0.05)
            : brand               ? 0
            : thinking            ? [0.08, 0.18, 0.08]
            : speaking            ? [0.12, 0.28, 0.12]
            :                        [0.05, 0.12, 0.05],
        }}
        transition={
          morph     ? { duration: 0.5, ease: deconstructing ? 'easeIn' : 'easeOut' }
          : brand   ? T_BRAND
          : speaking ? T_VOICE
          : { duration: thinking ? 5 : 8, repeat: Infinity, ease: 'easeInOut' }
        }
      />

      {/* ── Mid ambient ──────────────────────────────────────────────────────── */}
      <motion.circle
        cx={CX} cy={CY} r={90}
        fill="url(#rg-ambient)"
        animate={{
          opacity: deconstructing ? 0
            : reconstructing      ? (targetMode === 'brand' ? 0 : 0.10)
            : brand               ? 0
            : thinking            ? [0.12, 0.28, 0.12]
            : speaking            ? [0.22, 0.44, 0.22]
            :                        [0.10, 0.22, 0.10],
        }}
        transition={
          morph     ? { duration: 0.5, ease: deconstructing ? 'easeIn' : 'easeOut' }
          : brand   ? T_BRAND
          : speaking ? T_VOICE
          : { duration: thinking ? 4 : 6, repeat: Infinity, ease: 'easeInOut' }
        }
      />

      {/* ── Atmosphere ring ──────────────────────────────────────────────────── */}
      <motion.circle
        cx={CX} cy={CY} r={95}
        fill="none" stroke={C_PEAK} strokeWidth={22}
        filter="url(#f-atmosphere)"
        animate={{
          opacity: deconstructing ? 0
            : reconstructing      ? (targetMode === 'brand' ? 0 : 0.03)
            : brand               ? 0
            : thinking            ? [0.04, 0.10, 0.04]
            : speaking            ? [0.10, 0.22, 0.10]
            :                        [0.03, 0.08, 0.03],
        }}
        transition={
          morph     ? { duration: 0.5, ease: deconstructing ? 'easeIn' : 'easeOut' }
          : brand   ? T_BRAND
          : speaking ? T_VOICE
          : { duration: 7, repeat: Infinity, ease: 'easeInOut' }
        }
      />

      {/* ── Ring — shatters outward (fades) then re-forms last ───────────────── */}
      <motion.circle
        cx={CX} cy={CY} r={R_RING}
        fill="none" stroke={C_BASE} strokeWidth={2}
        animate={{
          opacity: deconstructing ? 0
            : reconstructing      ? (targetMode === 'brand' ? 0.55 : 0.75)
            : brand               ? 0.55
            : thinking            ? 0
            : speaking            ? [0.5, 1.0, 0.5]
            :                        [0.75, 0.35, 0.75],
          stroke: deconstructing  ? C_BASE
            : reconstructing      ? C_BASE
            : brand               ? C_BASE
            : thinking            ? C_BASE
            : speaking            ? C_PEAK
            :                        [C_PEAK, C_BASE, C_PEAK],
        }}
        transition={
          deconstructing ? { opacity: { duration: 0.45, ease: 'easeIn' },        stroke: { duration: 0 } }
          : reconstructing ? { opacity: { duration: 0.3, delay: 0.2, ease: 'easeOut' }, stroke: { duration: 0 } }
          : brand   ? T_BRAND
          : thinking ? { opacity: { duration: 1.5, ease: 'easeIn' }, stroke: { duration: 0.5 } }
          : speaking ? { opacity: { ...T_VOICE, delay: 1.2 }, stroke: { duration: 0.3 } }
          : T_RING
        }
      />

      {/* ── Spokes — cascade out (stagger per index), reverse cascade in ─────── */}
      {ITEMS.map(({ i, sx, sy, dx, dy }) => (
        <motion.line
          key={i}
          x1={sx} y1={sy} x2={dx} y2={dy}
          stroke={C_BASE} strokeWidth={1.5} strokeLinecap="round"
          animate={{
            opacity: deconstructing ? 0
              : reconstructing      ? 0.38
              : brand               ? 0.38
              : thinking            ? 0
              : speaking            ? 0.65
              :                        [0.38, 0.85, 0.38],
            stroke: deconstructing  ? C_BASE
              : reconstructing      ? C_BASE
              : brand               ? C_BASE
              : thinking            ? C_BASE
              : speaking            ? C_PEAK
              :                        [C_BASE, C_PEAK, C_BASE],
          }}
          transition={
            deconstructing
              ? { opacity: { duration: 0.35, delay: i * 0.03,       ease: 'easeIn'  }, stroke: { duration: 0 } }
              : reconstructing
              ? { opacity: { duration: 0.35, delay: (7 - i) * 0.03, ease: 'easeOut' }, stroke: { duration: 0 } }
              : brand   ? T_BRAND
              : thinking ? { opacity: { duration: 1.2, ease: 'easeIn' }, stroke: { duration: 0.3 } }
              : speaking ? { opacity: { duration: 1.0, delay: 0.8 }, stroke: { duration: 0.3, delay: 0.8 } }
              : T_SPOKES
          }
        />
      ))}

      {/* ── Speaking overlays ────────────────────────────────────────────────── */}
      <AnimatePresence>
        {speaking && (
          <motion.g
            key="speaking-overlays"
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
            transition={{ duration: 0.3, delay: 0.8 }}
          >
            {ITEMS.map(({ i, sx, sy, dx, dy }) => (
              <motion.line
                key={i}
                x1={sx} y1={sy} x2={dx} y2={dy}
                stroke={C_PEAK} strokeWidth={2} strokeLinecap="round"
                strokeDasharray={SPOKE_DASH}
                animate={{ strokeDashoffset: [0, SPOKE_DASH_END] }}
                transition={{ duration: 0.5, repeat: Infinity, ease: 'easeIn' }}
              />
            ))}
          </motion.g>
        )}
      </AnimatePresence>

      {/* ── Sound waves (speaking) ───────────────────────────────────────────── */}
      {WAVE_DELAYS.map((delay, i) => (
        <motion.circle
          key={`wave-${i}`}
          cx={CX} cy={CY} r={WAVE_R}
          fill="none" stroke={C_PEAK} strokeWidth={1.2}
          vectorEffect="non-scaling-stroke"
          filter="url(#f-wave)"
          style={SO}
          animate={
            speaking ? { scale: [WAVE_R_START, 1.0], opacity: [0.55, 0] }
            :           { scale: 0, opacity: 0 }
          }
          transition={speaking ? { duration: 1.5, repeat: Infinity, ease: 'easeOut', delay } : { duration: 0.4 }}
        />
      ))}

      {/* ── Autoencoder connection web (thinking only) ───────────────────────── */}
      <AnimatePresence>
        {thinking && (
          <motion.g
            key="autoencoder-web"
            initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
            transition={{ duration: 0.5, delay: 1.4 }}
          >
            {([...LEFT_CROSS_PAIRS, ...RIGHT_CROSS_PAIRS] as [number,number][]).map(([a, b], idx) => (
              <line
                key={`cross-${idx}`}
                x1={THINKING_POSITIONS[a][0]} y1={THINKING_POSITIONS[a][1]}
                x2={THINKING_POSITIONS[b][0]} y2={THINKING_POSITIONS[b][1]}
                stroke={C_BASE} strokeWidth={0.8} opacity={0.08}
              />
            ))}
            {[0,1,2,3].map(dotIdx => {
              const [x1, y1] = THINKING_POSITIONS[dotIdx]
              const lit = inputSignals.some(s => s.dotIdx === dotIdx)
              return (
                <motion.line
                  key={`in-${dotIdx}`}
                  x1={x1} y1={y1} x2={CX} y2={CY}
                  stroke={lit ? C_PEAK : C_BASE}
                  initial={{ opacity: 0.18, strokeWidth: 1 }}
                  animate={{ opacity: lit ? 0.85 : 0.18, strokeWidth: lit ? 2 : 1 }}
                  transition={{ duration: lit ? 0.4 : 0.6 }}
                />
              )
            })}
            {[4,5,6,7].map(dotIdx => {
              const [x2, y2] = THINKING_POSITIONS[dotIdx]
              const lit = outputSignals.some(s => s.dotIdx === dotIdx)
              return (
                <motion.line
                  key={`out-${dotIdx}`}
                  x1={CX} y1={CY} x2={x2} y2={y2}
                  stroke={lit ? C_PEAK : C_BASE}
                  initial={{ opacity: 0.18, strokeWidth: 1 }}
                  animate={{ opacity: lit ? 0.85 : 0.18, strokeWidth: lit ? 2 : 1 }}
                  transition={{ duration: lit ? 0.4 : 0.6 }}
                />
              )
            })}
          </motion.g>
        )}
      </AnimatePresence>

      {/* ── Signal orbs ──────────────────────────────────────────────────────── */}
      <AnimatePresence>
        {thinking && ([...inputSignals, ...outputSignals] as ForwardSignal[]).map(sig => (
          <SignalOrb key={sig.id} x1={sig.x1} y1={sig.y1} x2={sig.x2} y2={sig.y2} />
        ))}
      </AnimatePresence>

      {/* ── Outer dots — scatter outward then reassemble at target positions ─── */}
      {ITEMS.map(({ i, dx, dy, scatterX, scatterY }) => {
        const [thinkX, thinkY] = THINKING_POSITIONS[i]
        const idleFlare = !brand && !morph && state === 'idle'     && idleFiringDot === i
        const sigFlash  =          !morph && state === 'thinking'  && flashingDots.includes(i)

        return (
          <motion.g
            key={i}
            animate={{
              x: deconstructing ? scatterX : reconstructing ? 0 : thinking ? thinkX - dx : 0,
              y: deconstructing ? scatterY : reconstructing ? 0 : thinking ? thinkY - dy : 0,
            }}
            transition={{
              duration: morph ? 0.5 : 1.8,
              ease: deconstructing ? 'easeIn' : reconstructing ? 'easeOut' : 'easeInOut',
            }}
          >
            <motion.circle
              cx={dx} cy={dy} r={R_DOT}
              fill={C_BASE} style={SO}
              animate={{
                opacity: deconstructing ? 0.45
                  : reconstructing      ? (targetMode === 'brand' ? 0.75 : 0.85)
                  : brand               ? 0.75
                  :                        0.85,
                scale: !morph && speaking ? [1, 1.15, 1] : 1,
                fill:  !morph && speaking ? ([C_BASE, C_PEAK, C_BASE] as string[]) : C_BASE,
              }}
              transition={
                morph     ? { duration: 0.4, ease: deconstructing ? 'easeIn' : 'easeOut' }
                : brand   ? T_BRAND
                : speaking ? tDot(i)
                : { duration: 0.3 }
              }
            />
            <AnimatePresence>
              {(idleFlare || sigFlash) && (
                <motion.circle
                  key="flash"
                  cx={dx} cy={dy} r={R_DOT}
                  fill={C_PEAK}
                  initial={{ opacity: 1 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.7, ease: 'easeOut' }}
                />
              )}
            </AnimatePresence>
          </motion.g>
        )
      })}

      {/* ── Eye ring ─────────────────────────────────────────────────────────── */}
      <motion.circle
        cx={CX} cy={CY} r={R_EYE}
        fill="none" stroke={C_BASE} strokeWidth={2.5}
        filter="url(#f-core-glow)" style={SO}
        animate={{
          stroke:  deconstructing ? C_BASE
            : reconstructing      ? C_BASE
            : brand               ? C_BASE
            : thinking            ? C_PEAK
            : speaking            ? C_PEAK
            :                        [C_BASE, C_PEAK, C_BASE],
          opacity: deconstructing ? 0.2
            : reconstructing      ? (targetMode === 'brand' ? 0.65 : 0.55)
            : brand               ? 0.65
            : thinking            ? [0.55, 0.85, 0.55]
            : speaking            ? [0.60, 1.0,  0.60]
            :                        [0.55, 0.96, 0.55],
          scale:   deconstructing ? 0.88
            : reconstructing      ? 1.0
            : brand               ? 1
            : thinking            ? [1, 1.03, 1]
            : speaking            ? [1, 1.04, 1]
            :                        [1, 1.03, 1],
        }}
        transition={
          deconstructing ? { duration: 0.45, ease: 'easeIn' }
          : reconstructing ? { duration: 0.4, ease: 'easeOut' }
          : brand   ? T_BRAND
          : thinking ? { duration: 2, repeat: Infinity, ease: 'easeInOut' }
          : speaking ? T_VOICE
          : T_CORE
        }
      />

      {/* ── Pupil halo — dissolves during morph, reforms in animated mode ────── */}
      <motion.circle
        cx={CX} cy={CY} r={R_PUPIL_HALO}
        fill={C_PEAK} filter="url(#f-halo)" style={SO}
        animate={{
          opacity: deconstructing ? 0
            : reconstructing      ? (targetMode === 'brand' ? 0 : 0.22)
            : brand               ? 0
            : thinking            ? [0.08, 0.22, 0.08]
            : speaking            ? [0.30, 1.00, 0.30]
            :                        [0.22, 0.88, 0.22],
          scale:   deconstructing ? 0.8
            : reconstructing      ? 1.0
            : brand               ? 1
            : thinking            ? [0.85, 0.95, 0.85]
            : speaking            ? [1,    2.2,  1   ]
            :                        [1,    1.8,  1   ],
        }}
        transition={
          deconstructing ? { duration: 0.4, ease: 'easeIn' }
          : reconstructing ? { duration: 0.4, delay: 0.1, ease: 'easeOut' }
          : brand   ? T_BRAND
          : thinking ? { duration: 2, repeat: Infinity, ease: 'easeInOut' }
          : speaking ? T_VOICE
          : T_CORE
        }
      />

      {/* ── Pupil computation flash ───────────────────────────────────────────── */}
      {thinking && (
        <motion.circle
          key={`boost-${pupilBoostId}`}
          cx={CX} cy={CY} r={R_PUPIL_HALO * 1.6}
          fill={C_PEAK} filter="url(#f-halo)" style={SO}
          initial={{
            opacity: pupilBoost === 'deep' ? 0.88 : pupilBoost === 'standard' ? 0.62 : 0,
            scale: 1.0,
          }}
          animate={{ opacity: 0, scale: pupilBoost === 'deep' ? 3.0 : 2.2 }}
          transition={{ duration: pupilBoost === 'deep' ? 0.55 : 0.35, ease: 'easeOut' }}
        />
      )}

      {/* ── Pupil core — contracts slightly during deconstruct ───────────────── */}
      <motion.circle
        cx={CX} cy={CY} r={R_PUPIL}
        fill={C_BASE} filter="url(#f-core-glow)" style={SO}
        animate={{
          fill:  deconstructing ? C_BASE
            : reconstructing    ? C_BASE
            : brand             ? C_BASE
            : thinking          ? C_BASE
            : speaking          ? ([C_BASE, C_PEAK, C_BASE] as string[])
            :                      ([C_BASE, C_PEAK, C_BASE] as string[]),
          scale: deconstructing ? 0.7
            : reconstructing    ? 1.0
            : brand             ? 1
            : thinking          ? [0.85, 0.95, 0.85]
            : speaking          ? [1, 1.25, 1]
            :                      [1, 1.16, 1],
          opacity: deconstructing ? 0.6
            : reconstructing      ? (targetMode === 'brand' ? 0.85 : 1.0)
            : brand               ? 0.85
            :                        1,
        }}
        transition={
          deconstructing
            ? { scale: { duration: 0.45, ease: 'easeIn' }, opacity: { duration: 0.45 }, fill: { duration: 0 } }
            : reconstructing
            ? { scale: { duration: 0.4,  ease: 'easeOut' }, opacity: { duration: 0.4 }, fill: { duration: 0 } }
            : brand   ? T_BRAND
            : thinking ? { duration: 2, repeat: Infinity, ease: 'easeInOut' }
            : speaking ? T_VOICE
            : T_CORE
        }
      />
    </svg>
  )
}
