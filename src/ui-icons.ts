/** Toolbar and tab icons. Static markup only; no user text is interpolated. */
export type UiIconName =
  | "plus"
  | "minus"
  | "close"
  | "topology"
  | "servers"
  | "reload"
  | "gear"
  | "reconnect"
  | "terminal"
  | "planet"
  | "folder"
  | "file"
  | "upload"
  | "download"
  | "up";

const PATHS: Record<UiIconName, string> = {
  plus: `<path d="M8 3.5v9M3.5 8h9"/>`,
  minus: `<path d="M3.5 8h9"/>`,
  close: `<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>`,
  topology:
    `<rect x="6" y="1.75" width="4" height="3.5" rx="1"/>` +
    `<rect x="1.75" y="10.75" width="4" height="3.5" rx="1"/>` +
    `<rect x="10.25" y="10.75" width="4" height="3.5" rx="1"/>` +
    `<path d="M8 5.25v2.5M3.75 10.75V8.5a.75.75 0 0 1 .75-.75h7a.75.75 0 0 1 .75.75v2.25"/>`,
  servers:
    `<rect x="2" y="2.5" width="12" height="4.5" rx="1.25"/>` +
    `<rect x="2" y="9" width="12" height="4.5" rx="1.25"/>` +
    `<path d="M4.75 4.75h.01M4.75 11.25h.01"/>`,
  reload: `<path d="M13 8a5 5 0 1 1-1.46-3.54M13 2.75v2.5h-2.5"/>`,
  gear:
    `<circle cx="8" cy="8" r="2"/>` +
    `<path d="M8 1.75v1.5M8 12.75v1.5M1.75 8h1.5M12.75 8h1.5M3.58 3.58l1.06 1.06M11.36 11.36l1.06 1.06M3.58 12.42l1.06-1.06M11.36 4.64l1.06-1.06"/>` +
    `<circle cx="8" cy="8" r="4.25"/>`,
  reconnect: `<path d="M2.75 8a5.25 5.25 0 0 1 9-3.7M13.25 8a5.25 5.25 0 0 1-9 3.7M11.75 1.75v2.75H9M4.25 14.25V11.5H7"/>`,
  planet: `<circle cx="8" cy="8" r="3.5"/><ellipse cx="8" cy="8" rx="7" ry="2.4" transform="rotate(-18 8 8)"/>`,
  folder: `<path d="M1.75 4.25a1 1 0 0 1 1-1h3.1l1.4 1.5h5.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z"/>`,
  file: `<path d="M4 1.75h5.25l2.75 2.75v9.75H4z"/><path d="M9.25 1.75V4.5H12"/>`,
  upload: `<path d="M8 10.5V2.75M4.75 6 8 2.75 11.25 6M2.75 13.25h10.5"/>`,
  download: `<path d="M8 2.75v7.75M4.75 7.25 8 10.5l3.25-3.25M2.75 13.25h10.5"/>`,
  up: `<path d="M8 13V3.25M3.75 7.5 8 3.25l4.25 4.25"/>`,
  terminal: `<rect x="1.75" y="2.75" width="12.5" height="10.5" rx="1.75"/><path d="M4.5 6.25 6.75 8 4.5 9.75M8.5 10h3"/>`,
};

export function uiIcon(name: UiIconName): string {
  return (
    `<svg class="ui-icon" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" ` +
    `stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${PATHS[name]}</svg>`
  );
}
