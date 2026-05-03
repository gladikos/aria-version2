import type { ChatMessage } from '../hooks/useChat'

export interface MockChat {
  id:          string
  title:       string
  lastMessage: string
  updatedAt:   Date
  folderId:    string | null
}

const ago = (ms: number) => new Date(Date.now() - ms)
const h   = (n: number)  => ago(n * 3_600_000)
const d   = (n: number)  => ago(n * 86_400_000)

export const MOCK_CHATS: MockChat[] = [
  {
    id: 'chat-1', folderId: null,
    title:       'Finding the aria project',
    lastMessage: "It's at D:\\personal-dev\\aria-v2 — you set that up last Tuesday.",
    updatedAt:   h(2),
  },
  {
    id: 'chat-2', folderId: 'f2',
    title:       'Tame Impala new album',
    lastMessage: 'Their new album Deadbeat dropped last Friday. The lead single has that classic Kevin Parker...',
    updatedAt:   h(5),
  },
  {
    id: 'chat-3', folderId: 'f3',
    title:       'Greek politics headlines',
    lastMessage: 'The opposition pushed a no-confidence motion. Mitsotakis survived with a narrow margin...',
    updatedAt:   d(1),
  },
  {
    id: 'chat-4', folderId: 'f1',
    title:       'React performance profiling',
    lastMessage: 'Wrap the message list in React.memo and make the scroll effect depend only on message count.',
    updatedAt:   d(2),
  },
  {
    id: 'chat-5', folderId: 'f2',
    title:       'Weekend dinner ideas',
    lastMessage: 'Seared duck breast with cherry reduction. Takes about 30 minutes. Score the fat...',
    updatedAt:   d(3),
  },
  {
    id: 'chat-6', folderId: 'f3',
    title:       'Stoic philosophy reading list',
    lastMessage: "Start with Meditations by Marcus Aurelius, then Epictetus' Enchiridion. Both are short.",
    updatedAt:   d(4),
  },
  {
    id: 'chat-7', folderId: 'f1',
    title:       'TypeScript generics deep dive',
    lastMessage: 'Use conditional types with infer to extract the return type of any async function cleanly.',
    updatedAt:   d(5),
  },
  {
    id: 'chat-8', folderId: 'f3',
    title:       'Climate data analysis',
    lastMessage: 'The NOAA dataset has monthly averages back to 1880. Load it with pandas, resample to annual.',
    updatedAt:   d(6),
  },
  {
    id: 'chat-9', folderId: 'f1',
    title:       'Tauri Rust integration',
    lastMessage: 'Define your command with #[tauri::command] and register it in tauri::Builder::invoke_handler.',
    updatedAt:   d(8),
  },
  {
    id: 'chat-10', folderId: null,
    title:       'Morning routine ideas',
    lastMessage: '20 minutes of reading before any screens makes a real difference. Try it for a week.',
    updatedAt:   d(10),
  },
]

export const MOCK_MESSAGES: Record<string, ChatMessage[]> = {
  'chat-1': [
    { id: 'c1m1', role: 'user', content: 'Where is my aria project?' },
    { id: 'c1m2', role: 'aria', content: "It's at `D:\\personal-dev\\aria-v2` — you set that up last Tuesday. Want me to open it?" },
  ],
  'chat-2': [
    { id: 'c2m1', role: 'user', content: 'Has Tame Impala released anything new?' },
    { id: 'c2m2', role: 'aria', content: "Their new album **Deadbeat** dropped last Friday. The lead single has that classic Kevin Parker layered production — lots of synth textures over a driving beat. The album leans more electronic than *Currents*." },
  ],
  'chat-3': [
    { id: 'c3m1', role: 'user', content: "What's happening in Greek politics?" },
    { id: 'c3m2', role: 'aria', content: "The opposition pushed a no-confidence motion last week. Mitsotakis survived with a narrow margin. SYRIZA's new leadership has been consolidating support ahead of the next cycle." },
  ],
  'chat-4': [
    { id: 'c4m1', role: 'user', content: 'My React app feels slow. How do I profile it?' },
    { id: 'c4m2', role: 'aria', content: "Open Chrome DevTools → Performance tab → record a session. Look for long tasks in the flame graph, and components re-rendering without prop changes — use React DevTools Profiler for that." },
    { id: 'c4m3', role: 'user', content: "Found it — the transcript re-renders on every token." },
    { id: 'c4m4', role: 'aria', content: "Classic streaming UI problem. Wrap the message list in `React.memo` and make the scroll effect depend only on message count, not message content. That should eliminate the cascade." },
  ],
  'chat-5': [
    { id: 'c5m1', role: 'user', content: "What should I cook this weekend? Something impressive but not too hard." },
    { id: 'c5m2', role: 'aria', content: "Seared duck breast with cherry reduction. Takes about 30 minutes. Score the fat in a crosshatch, render skin-side down for 12 minutes, flip for 3, rest for 5. Deglaze with port and cherries while it rests." },
  ],
  'chat-6': [
    { id: 'c6m1', role: 'user', content: 'Give me a Stoic philosophy reading list.' },
    { id: 'c6m2', role: 'aria', content: "Start with **Meditations** by Marcus Aurelius, then Epictetus' **Enchiridion**. Both are short. Follow with Seneca's **Letters from a Stoic** for practical guidance. If you want the theory, Cicero's **Tusculan Disputations** fills the gaps." },
  ],
  'chat-7': [
    { id: 'c7m1', role: 'user', content: 'Explain TypeScript conditional types and infer.' },
    { id: 'c7m2', role: 'aria', content: "Use conditional types with `infer` to extract the return type of any async function cleanly: `type Awaited<T> = T extends Promise<infer R> ? R : T`. The `infer` keyword captures a type variable within the extends clause." },
  ],
  'chat-8': [
    { id: 'c8m1', role: 'user', content: 'How do I load NOAA climate data in Python?' },
    { id: 'c8m2', role: 'aria', content: "The NOAA dataset has monthly averages back to 1880. Load it with pandas (`pd.read_csv`), parse the date column, then resample to annual means with `.resample('YE').mean()`. Plot with matplotlib or plotly for a quick visual." },
  ],
  'chat-9': [
    { id: 'c9m1', role: 'user', content: 'How do I expose a Rust function to JavaScript in Tauri?' },
    { id: 'c9m2', role: 'aria', content: "Define your command with `#[tauri::command]` and register it in `tauri::Builder::invoke_handler(tauri::generate_handler![your_fn])`. On the JS side, call it with `invoke('your_fn', { arg: value })` from `@tauri-apps/api/core`." },
  ],
  'chat-10': [
    { id: 'c10m1', role: 'user', content: 'How do I build a better morning routine?' },
    { id: 'c10m2', role: 'aria', content: "20 minutes of reading before any screens makes a real difference. Anchor it to something you already do — coffee, breakfast. Try it for a week before adding anything else. Small and consistent beats ambitious and abandoned." },
  ],
}
