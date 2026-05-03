import type { SVGProps } from 'react'

const CX          = 100
const CY          = 100
const R_RING      = 74
const R_EYE       = 19
const R_PUPIL     = 8
const R_DOT       = 7.5
const SPOKE_START = R_EYE + 2
const C           = '#3A8AAA'

function polar(r: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180
  return [CX + r * Math.cos(rad), CY + r * Math.sin(rad)]
}

const ITEMS = Array.from({ length: 8 }, (_, i) => {
  const deg      = i * 45
  const [sx, sy] = polar(SPOKE_START, deg)
  const [dx, dy] = polar(R_RING, deg)
  return { i, sx, sy, dx, dy }
})

export default function BrandLogo(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 200 200"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Aria"
      role="img"
      {...props}
    >
      {/* Outer ring */}
      <circle cx={CX} cy={CY} r={R_RING} fill="none" stroke={C} strokeWidth={2} opacity={0.55} />

      {/* Spokes */}
      {ITEMS.map(({ i, sx, sy, dx, dy }) => (
        <line key={i} x1={sx} y1={sy} x2={dx} y2={dy}
          stroke={C} strokeWidth={1.5} strokeLinecap="round" opacity={0.38} />
      ))}

      {/* Outer dots */}
      {ITEMS.map(({ i, dx, dy }) => (
        <circle key={i} cx={dx} cy={dy} r={R_DOT} fill={C} opacity={0.75} />
      ))}

      {/* Eye ring */}
      <circle cx={CX} cy={CY} r={R_EYE} fill="none" stroke={C} strokeWidth={2.5} opacity={0.65} />

      {/* Pupil */}
      <circle cx={CX} cy={CY} r={R_PUPIL} fill={C} opacity={0.85} />
    </svg>
  )
}
