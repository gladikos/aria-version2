import type { SVGProps } from 'react'

const CX = 100
const CY = 100
const R_RING = 74        // radius to dot centres / ring
const R_EYE = 19         // eye circle radius
const R_PUPIL = 8        // pupil radius
const R_DOT = 7.5        // outer dot radius
const SPOKE_START = R_EYE + 2   // leave a small gap after the eye stroke

function polar(r: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180
  return [CX + r * Math.cos(rad), CY + r * Math.sin(rad)]
}

export default function AriaLogo(props: SVGProps<SVGSVGElement>) {
  const items = Array.from({ length: 8 }, (_, i) => {
    const deg = i * 45
    const [sx, sy] = polar(SPOKE_START, deg)
    const [dx, dy] = polar(R_RING, deg)
    return { i, sx, sy, dx, dy }
  })

  return (
    <svg
      viewBox="0 0 200 200"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Aria"
      role="img"
      {...props}
    >
      {/* Ring — structural; lighter via opacity */}
      <circle
        id="ring"
        cx={CX} cy={CY} r={R_RING}
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        opacity={0.6}
      />

      {/* Spokes — structural; lighter via opacity */}
      {items.map(({ i, sx, sy, dx, dy }) => (
        <line
          key={i}
          id={`spoke-${i}`}
          x1={sx} y1={sy} x2={dx} y2={dy}
          stroke="currentColor"
          strokeWidth={1.5}
          strokeLinecap="round"
          opacity={0.6}
        />
      ))}

      {/* Eye — painted over spoke starts */}
      <circle
        id="eye"
        cx={CX} cy={CY} r={R_EYE}
        fill="none"
        stroke="currentColor"
        strokeWidth={2.5}
        opacity={0.6}
      />

      {/* Outer dots — solid; painted over spoke ends + ring */}
      {items.map(({ i, dx, dy }) => (
        <circle
          key={i}
          id={`dot-${i}`}
          cx={dx} cy={dy} r={R_DOT}
          fill="currentColor"
        />
      ))}

      {/* Pupil — solid; painted over eye */}
      <circle
        id="pupil"
        cx={CX} cy={CY} r={R_PUPIL}
        fill="currentColor"
      />
    </svg>
  )
}
