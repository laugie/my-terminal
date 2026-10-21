/** Inline diagram symbols. Geometric marks only; no external artwork. */
export function iconSymbols(): string {
  return [
    symbol(
      "router",
      `<path d="M16 11v6M32 11v6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>` +
        `<circle cx="16" cy="8" r="2" fill="currentColor"/>` +
        `<circle cx="32" cy="8" r="2" fill="currentColor"/>` +
        `<rect x="8" y="17" width="32" height="18" rx="3" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<circle cx="16" cy="26" r="1.6" fill="currentColor"/>` +
        `<circle cx="22" cy="26" r="1.6" fill="currentColor"/>` +
        `<path d="M28 26h8" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>`,
    ),
    symbol(
      "nas",
      `<rect x="9" y="10" width="30" height="28" rx="3" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<ellipse cx="24" cy="19" rx="8" ry="3" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<ellipse cx="24" cy="27" rx="8" ry="3" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<circle cx="33" cy="33" r="1.6" fill="currentColor"/>`,
    ),
    symbol(
      "mac",
      `<rect x="12" y="8" width="24" height="16" rx="2" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<path d="M7 28h34l-4 7H11z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>`,
    ),
    symbol(
      "windows",
      `<rect x="8" y="8" width="32" height="22" rx="2" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<path d="M24 30v6M16 38h16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>`,
    ),
    symbol(
      "cloud",
      `<path d="M17 34h16.5a7.5 7.5 0 0 0 1-14.9 9.5 9.5 0 0 0-18.3-2A6.5 6.5 0 0 0 17 34z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/>`,
    ),
    symbol(
      "server",
      `<rect x="11" y="6" width="26" height="36" rx="2" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<path d="M11 18h26M11 30h26" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<circle cx="17" cy="12" r="1.5" fill="currentColor"/>` +
        `<circle cx="17" cy="24" r="1.5" fill="currentColor"/>` +
        `<circle cx="17" cy="36" r="1.5" fill="currentColor"/>`,
    ),
    symbol(
      "generic",
      `<rect x="10" y="10" width="28" height="28" rx="6" fill="none" stroke="currentColor" stroke-width="2"/>` +
        `<circle cx="24" cy="24" r="4" fill="none" stroke="currentColor" stroke-width="2"/>`,
    ),
  ].join("");
}

function symbol(kind: string, body: string): string {
  return `<symbol id="diagram-icon-${kind}" viewBox="0 0 48 48">${body}</symbol>`;
}
