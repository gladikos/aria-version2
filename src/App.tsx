import AriaLogo from './components/AriaLogo'
import { useAriaState } from './hooks/useAriaState'

export default function App() {
  const { state } = useAriaState()

  return (
    <div
      className="flex items-center justify-center h-full w-full"
      style={{ backgroundColor: '#0A0E14', color: '#3A8AAA' }}
    >
      <AriaLogo state={state} width={400} height={400} />
    </div>
  )
}
