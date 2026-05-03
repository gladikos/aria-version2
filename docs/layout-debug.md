# Layout Debug — Ghost Area Diagnosis

## Adapted console script
Run this in Tauri DevTools (right-click app → Inspect, or Ctrl+Shift+I → Console tab).
Selectors are corrected for the actual DOM structure (codebase uses inline styles, almost no class names).

```js
const log = (label, el) => {
  if (!el) return `${label}: NOT FOUND`;
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  return `${label}:\n  rect: top=${r.top} left=${r.left} w=${r.width} h=${r.height} bottom=${r.bottom}\n  computed: height=${cs.height} minHeight=${cs.minHeight} maxHeight=${cs.maxHeight}\n  overflow=${cs.overflow} overflowY=${cs.overflowY} position=${cs.position}\n  flex=${cs.flex} display=${cs.display}`;
};

const root     = document.querySelector('#root');
const appRoot  = root?.firstElementChild;              // App's outer div
const titleBar = document.querySelector('[data-tauri-drag-region]');
const mainArea = titleBar?.nextElementSibling;         // flex:1 content row
const chatCol  = document.querySelector('.chat-column');
const chatOuter= chatCol?.firstElementChild;           // ChatPanel outer div (minHeight:0)
const glass    = chatOuter?.firstElementChild;         // glass panel (backdrop-filter)
const transcript = document.querySelector('.transcript-scroll');
const inputRow = glass?.lastElementChild;              // input row div

console.log([
  log('html',        document.documentElement),
  log('body',        document.body),
  log('#root',       root),
  log('app-root',    appRoot),
  log('title-bar',   titleBar),
  log('main-area',   mainArea),
  log('.chat-column',chatCol),
  log('chat-outer',  chatOuter),
  log('glass-panel', glass),
  log('.transcript-scroll', transcript),
  log('input-row',   inputRow),
].join('\n\n'));

console.log('window.innerHeight =', window.innerHeight);
console.log('window.outerHeight =', window.outerHeight);
console.log('screen.height =', screen.height);
console.log('screen.availHeight =', screen.availHeight);
console.log('devicePixelRatio =', window.devicePixelRatio);
console.log('document.body.scrollHeight =', document.body.scrollHeight);
console.log('document.documentElement.scrollHeight =', document.documentElement.scrollHeight);
console.log('document.documentElement.clientHeight =', document.documentElement.clientHeight);
```

---

## Results

*(paste console output here)*

```
PASTE OUTPUT HERE
```

---

## Screenshots needed

1. DevTools Elements panel — hover from `<html>` down, find the element whose highlight extends below the visible window
2. DevTools Layout/Box Model tab for the offending element

---

## Hypothesis to verify

**Tauri window taller than visible area (DPI scaling)?**
If `window.innerHeight` > `document.documentElement.clientHeight`, or if the reported pixel height doesn't match what you see on screen, the ghost is the window itself overflowing the taskbar/screen — not a CSS issue.

Check: `window.innerHeight` vs actual visible height in pixels.
If `innerHeight = 800` but the app visually occupies only ~700px, the fix is in `tauri.conf.json` (window height) or a DPI scaling misconfiguration with `decorations: false`.

**What to look for in the Elements panel:**
- Blue highlight = content box
- Green highlight = padding
- Orange highlight = margin
- The element whose highlight extends below the taskbar / below what's rendered = the ghost source
