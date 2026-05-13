// brand-logos.js — logo.dev API with monogram fallback
// Served at /js/brand-logos.js — load via <script src="/js/brand-logos.js"></script>
// Exposes: window.BrandLogos = { brandLogoUrl, brandDomain, monogramElement, brandLogoElement }

(function (w) {
  'use strict';

  const LOGO_DEV_BASE = 'https://img.logo.dev';

  // Map: known brand/institution name → canonical domain logo.dev resolves.
  // null = personal or no-brand item; force monogram.
  const BRAND_DOMAIN_MAP = {
    // Banks
    'Piraeus Bank':         'piraeusbank.gr',
    'Revolut':              'revolut.com',
    'National Bank of Greece': 'nbg.gr',
    'Eurobank':             'eurobank.gr',
    'Alpha Bank':           'alpha.gr',
    'Mock ASPSP':           null,

    // Investments
    'NN Accelerator+':      'nnhellas.gr',
    'NN Hellas':            'nnhellas.gr',
    'NN':                   'nnhellas.gr',

    // Subscriptions
    'Netflix':              'netflix.com',
    'Spotify':              'spotify.com',
    'Disney+':              'disneyplus.com',
    'Disney Plus':          'disneyplus.com',
    'Claude Max':           'anthropic.com',
    'Claude Pro':           'anthropic.com',
    'Anthropic':            'anthropic.com',
    'Anthropic API':        'anthropic.com',
    'GitHub Copilot':       'github.com',
    'Copilot':              'github.com',
    'ElevenLabs':           'elevenlabs.io',
    'ElevenLabs Creator':   'elevenlabs.io',
    'ElevenLabs Starter':   'elevenlabs.io',
    'Brave Search':         'brave.com',
    'Brave':                'brave.com',
    'YouTube Premium':      'youtube.com',
    'YouTube':              'youtube.com',
    'Apple':                'apple.com',
    'iCloud':               'apple.com',
    'Google':               'google.com',
    'Google One':           'google.com',
    'Gemini':               'google.com',
    'Microsoft 365':        'microsoft.com',
    'Office 365':           'microsoft.com',
    'Dropbox':              'dropbox.com',
    'Adobe':                'adobe.com',
    'Figma':                'figma.com',
    'Notion':               'notion.so',
    'Linear':               'linear.app',
    'Slack':                'slack.com',
    'Tennis':               null,
    'Tennis Lessons':       null,
  };

  function getToken() {
    return w.__LOGO_DEV_TOKEN__ || '';
  }

  // Returns the canonical domain for a brand name, or null if unknown/personal.
  // Does exact match first, then case-insensitive substring.
  function brandDomain(brandName) {
    if (!brandName) return null;
    if (Object.prototype.hasOwnProperty.call(BRAND_DOMAIN_MAP, brandName)) {
      return BRAND_DOMAIN_MAP[brandName];
    }
    const lower = brandName.toLowerCase();
    const key = Object.keys(BRAND_DOMAIN_MAP).find(k =>
      lower.includes(k.toLowerCase()) || k.toLowerCase().includes(lower)
    );
    return key ? BRAND_DOMAIN_MAP[key] : null;
  }

  // Returns the logo.dev URL string, or null if token or domain is missing.
  function brandLogoUrl({ domain, size = 64 }) {
    const token = getToken();
    if (!domain || !token) return null;
    const params = new URLSearchParams({
      token,
      size: String(Math.round(size)),
      retina: 'true',
      format: 'webp',
    });
    return `${LOGO_DEV_BASE}/${domain}?${params.toString()}`;
  }

  // Returns a div monogram element (letter on subtle dark background).
  function monogramElement(brandName, size) {
    size = size || 48;
    const letter = (brandName || '?').trim().charAt(0).toUpperCase();
    const el = document.createElement('div');
    el.className = 'brand-monogram';
    el.style.cssText = [
      'display:inline-flex',
      'align-items:center',
      'justify-content:center',
      'border-radius:50%',
      'background:linear-gradient(135deg,#2a2f3a 0%,#1a1d24 100%)',
      'border:1px solid rgba(255,255,255,0.10)',
      'color:#c8d0dc',
      'font-weight:500',
      'font-family:system-ui,-apple-system,sans-serif',
      'letter-spacing:-0.5px',
      'flex-shrink:0',
      'box-sizing:border-box',
      'width:' + size + 'px',
      'height:' + size + 'px',
      'font-size:' + Math.round(size * 0.40) + 'px',
    ].join(';');
    el.textContent = letter;
    return el;
  }

  // Returns an <img> that loads from logo.dev, falling back to monogram on error.
  // If no token or domain, returns monogram immediately.
  function brandLogoElement(brandName, size) {
    size = size || 48;
    const domain = brandDomain(brandName);
    const token  = getToken();

    if (!domain || !token) {
      return monogramElement(brandName, size);
    }

    const img = document.createElement('img');
    img.className = 'brand-logo';
    img.width  = size;
    img.height = size;
    img.alt    = brandName || '';
    img.style.cssText = [
      'width:' + size + 'px',
      'height:' + size + 'px',
      'border-radius:50%',
      'background:rgba(255,255,255,0.04)',
      'object-fit:contain',
      'padding:4px',
      'box-sizing:border-box',
      'flex-shrink:0',
    ].join(';');
    img.src = brandLogoUrl({ domain, size: size * 2 });
    img.addEventListener('error', function () {
      img.replaceWith(monogramElement(brandName, size));
    }, { once: true });
    return img;
  }

  w.BrandLogos = { brandLogoUrl, brandDomain, monogramElement, brandLogoElement };
})(window);
