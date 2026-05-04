/**
 * Aria browser sidecar — JSON-over-stdio protocol.
 * Connects to the user's Chrome via CDP (--remote-debugging-port=9222).
 *
 * Reads:  {"id": "<uuid>", "method": "<name>", "params": {...}}  (one per line on stdin)
 * Writes: {"id": "<uuid>", "result": ...}                        (one per line on stdout)
 *      or {"id": "<uuid>", "error": "<message>"}
 *
 * stderr is for human-readable diagnostics only (Rust side logs it but does not parse it).
 */

import { chromium } from 'playwright';
import * as readline from 'readline';
import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';

// ─── Browser state ────────────────────────────────────────────────────────────

const CDP_ENDPOINT = 'http://localhost:9222';

let browser = null;
let context  = null;
let page     = null;

async function ensureBrowser() {
  if (browser && context && page && !page.isClosed()) return;

  let lastErr;
  for (let attempt = 1; attempt <= 5; attempt++) {
    try {
      browser = await chromium.connectOverCDP(CDP_ENDPOINT);
      const contexts = browser.contexts();
      context = contexts[0];
      if (!context) throw new Error('No browser context found after CDP connect');
      const pages = context.pages();
      page = pages.find(p => !p.isClosed()) || await context.newPage();
      process.stderr.write(
        `[sidecar] connected to Chrome via CDP (attempt ${attempt}, ${pages.length} existing pages)\n`
      );
      return;
    } catch (e) {
      lastErr = e;
      process.stderr.write(`[sidecar] CDP attempt ${attempt}/5 failed: ${e.message}\n`);
      if (attempt < 5) await new Promise(r => setTimeout(r, 1000));
    }
  }

  throw new Error(
    `Aria's browser isn't running yet (tried ${CDP_ENDPOINT} 5 times). ` +
    `Call launch_aria_chrome to start it.`
  );
}

// ─── Consent banner dismissal ─────────────────────────────────────────────────

// Searches both the main page and all iframes. Prefers "Reject all" over "Accept all".
async function dismissConsentBanners(pg) {
  const labels = ['Reject all', 'Accept all', 'I agree', 'Accept cookies', 'Got it'];
  const targets = [pg, ...pg.frames()];

  for (const target of targets) {
    for (const label of labels) {
      const selectors = [
        `button:has-text("${label}")`,
        `[role="button"]:has-text("${label}")`,
        `tp-yt-paper-button:has-text("${label}")`,
        `[aria-label="${label}"]`,
        `button[aria-label*="${label}"]`,
      ];
      for (const sel of selectors) {
        try {
          const el = target.locator(sel).first();
          if (await el.isVisible({ timeout: 300 })) {
            await el.click({ timeout: 1500 });
            process.stderr.write(`[sidecar] dismissed banner: "${label}" via ${sel}\n`);
            await pg.waitForTimeout(500);
            return true;
          }
        } catch { /* try next */ }
      }
    }
  }

  process.stderr.write('[sidecar] no consent banner found\n');
  return false;
}

// ─── Method implementations ───────────────────────────────────────────────────

const METHODS = {
  async start() {
    await ensureBrowser();
    return { ok: true };
  },

  async navigate({ url }) {
    await ensureBrowser();
    // Always open a new tab — preserves whatever the user had open
    page = await context.newPage();
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 });
    await page.waitForTimeout(1500); // banners often appear after a short delay
    await dismissConsentBanners(page);
    await page.waitForTimeout(800); // let JS-heavy pages settle after consent dismissal
    return { url: page.url(), title: await page.title() };
  },

  async click({ selector }) {
    await ensureBrowser();
    await page.click(selector, { timeout: 10000 });
    return { ok: true, selector };
  },

  async type_text({ selector, text, submit = false }) {
    await ensureBrowser();
    await page.fill(selector, String(text), { timeout: 10000 });
    if (submit) {
      await page.press(selector, 'Enter');
    }
    return { ok: true };
  },

  async get_page_text({ max_chars = 5000 } = {}) {
    await ensureBrowser();
    const text = await page.evaluate(
      () => document.body?.innerText ?? ''
    );
    return text.slice(0, max_chars);
  },

  async screenshot() {
    await ensureBrowser();
    const dir = path.join(os.tmpdir(), 'aria_screenshots');
    fs.mkdirSync(dir, { recursive: true });
    const filepath = path.join(dir, `screenshot_${Date.now()}.png`);
    await page.screenshot({ path: filepath, fullPage: false });
    return { filepath };
  },

  async scroll({ direction, amount = 500 }) {
    await ensureBrowser();
    switch (direction) {
      case 'top':
        await page.evaluate(() => window.scrollTo(0, 0));
        break;
      case 'bottom':
        await page.evaluate(
          () => window.scrollTo(0, document.body.scrollHeight)
        );
        break;
      case 'up':
        await page.evaluate((amt) => window.scrollBy(0, -amt), amount);
        break;
      default: // 'down'
        await page.evaluate((amt) => window.scrollBy(0, amt), amount);
    }
    return { ok: true, direction, amount };
  },

  async wait_for_selector({ selector, timeout = 15000 }) {
    await ensureBrowser();
    try {
      await page.waitForSelector(selector, { timeout });
    } catch (e) {
      process.stderr.write(`[sidecar] wait_for_selector timeout — trying banner dismiss + retry\n`);
      await dismissConsentBanners(page);
      await page.waitForSelector(selector, { timeout: 5000 });
    }
    return { ok: true, selector };
  },

  async current_url() {
    if (!page || page.isClosed()) return { url: null };
    return { url: page.url() };
  },

  // CDP: browser.close() disconnects only — does not shut down the user's Chrome
  async close() {
    if (browser) {
      await browser.close();
      browser  = null;
      context  = null;
      page     = null;
      process.stderr.write('[sidecar] disconnected from Chrome\n');
    }
    return { ok: true };
  },
};

// ─── Protocol helpers ─────────────────────────────────────────────────────────

function respond(id, result) {
  process.stdout.write(JSON.stringify({ id, result }) + '\n');
}

function respondError(id, err) {
  const message = (err && err.message) ? err.message : String(err);
  process.stdout.write(JSON.stringify({ id, error: message }) + '\n');
}

// ─── Stdin dispatch loop ──────────────────────────────────────────────────────

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });

rl.on('line', async (raw) => {
  const line = raw.trim();
  if (!line) return;

  let req;
  try {
    req = JSON.parse(line);
  } catch {
    process.stderr.write(`[sidecar] bad JSON on stdin: ${line}\n`);
    return;
  }

  const { id, method, params = {} } = req;
  if (!id || !method) {
    process.stderr.write(`[sidecar] missing id or method: ${line}\n`);
    return;
  }

  const handler = METHODS[method];
  if (!handler) {
    respondError(id, `Unknown method: ${method}`);
    return;
  }

  try {
    const result = await handler(params);
    respond(id, result);
  } catch (err) {
    process.stderr.write(`[sidecar] ${method} error: ${err?.message ?? err}\n`);
    respondError(id, err);
  }
});

rl.on('close', () => {
  process.stderr.write('[sidecar] stdin closed — shutting down\n');
  if (browser) browser.close().catch(() => {});
  process.exit(0);
});

process.stderr.write('[sidecar] ready\n');
