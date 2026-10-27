import { iconSymbols } from "./icons.js";
import { TAB_LIMIT_MESSAGE } from "./tabs.js";
import type { Presence, TopologyView } from "./topology.js";

export type DeviceKind = "server" | "mac" | "windows" | "router" | "nas" | "cloud" | "generic";

const NAMED_KINDS: readonly DeviceKind[] = ["server", "mac", "windows", "router", "nas", "cloud"];

/** Category order is the match priority. Latin keywords are whole tokens. */
const KEYWORDS: ReadonlyArray<readonly [DeviceKind, readonly string[]]> = [
  ["router", ["router", "gw", "gateway", "openwrt", "ap", "路由"]],
  ["nas", ["nas", "synology", "qnap", "truenas", "群晖", "存储"]],
  ["mac", ["mac", "macbook", "imac", "mbp"]],
  ["windows", ["win", "windows"]],
  ["cloud", ["vps", "cloud", "ec2", "ecs", "lightsail", "droplet", "云"]],
  ["server", ["server", "srv", "db", "app", "web", "服务器"]],
];

/** Layout width when the caller does not pass one (tests, preview script). */
const DEFAULT_WIDTH = 1280;
const MIN_WIDTH = 360;
/** One device cell. */
const NODE_W = 156;
const NODE_H = 132;
/** A stretched region spreads its cells, but never wider than this. */
const MAX_CELL_W = 240;
const PAD_X = 16;
const PAD_TOP = 52;
const PAD_BOTTOM = 12;
const GAP = 24;
const MARGIN = 24;
/** Rounded tile behind each device icon. */
const TILE = 60;
const TILE_X = (NODE_W - TILE) / 2;
const TILE_Y = 12;
const ICON = 40;
const NAME_Y = TILE_Y + TILE + 22;
const ADDRESS_Y = NAME_Y + 19;
const LABEL_BOTTOM = ADDRESS_Y + 8;
const OPEN_LIMIT_HINT = TAB_LIMIT_MESSAGE;

export interface DiagramGroup {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface DiagramNode {
  name: string;
  region: string;
  x: number;
  y: number;
  kind: DeviceKind;
  host: string;
  port: number;
  mobile: boolean;
  presence: Presence;
}

export interface DiagramEdge {
  from: string;
  to: string;
  crossRegion: boolean;
}

export interface DiagramModel {
  groups: DiagramGroup[];
  nodes: DiagramNode[];
  edges: DiagramEdge[];
  width: number;
  height: number;
}

export function deviceKind(device: { name: string; type?: unknown }): DeviceKind {
  const raw = device.type;
  if (raw !== undefined && raw !== null) {
    if (typeof raw !== "string") {
      return "generic";
    }
    const normalized = raw.trim().toLowerCase();
    if (isNamedKind(normalized)) {
      return normalized;
    }
    return "generic";
  }
  return guessKind(device.name);
}

export function buildDiagram(view: TopologyView, options?: { width?: number }): DiagramModel {
  const width = Math.max(MIN_WIDTH, Math.floor(options?.width || DEFAULT_WIDTH));
  const inner = width - MARGIN * 2;
  const maxCols = Math.max(1, Math.floor((inner - PAD_X * 2) / NODE_W));
  const sized = view.regions.map((region) => {
    const count = region.devices.length;
    const cols = Math.max(1, Math.min(count, maxCols));
    const rows = Math.ceil(count / cols);
    return {
      region,
      cols,
      width: PAD_X * 2 + cols * NODE_W,
      height: PAD_TOP + PAD_BOTTOM + rows * NODE_H,
    };
  });

  // Regions flow left to right in file order and wrap at the available width.
  const rows: Array<typeof sized> = [];
  let used = 0;
  for (const item of sized) {
    const current = rows[rows.length - 1];
    if (current && used + GAP + item.width <= inner) {
      current.push(item);
      used += GAP + item.width;
      continue;
    }
    rows.push([item]);
    used = item.width;
  }

  const groups: DiagramGroup[] = [];
  const nodes: DiagramNode[] = [];
  let cursorY = MARGIN;
  let maxX = MARGIN;
  for (const row of rows) {
    // Each row is stretched to the full width, shared out by natural width.
    const natural = row.reduce((sum, item) => sum + item.width, 0);
    const extra = Math.max(0, inner - natural - GAP * (row.length - 1));
    const height = Math.max(...row.map((item) => item.height));
    let cursorX = MARGIN;
    let grown = 0;
    row.forEach((item, index) => {
      const share =
        index === row.length - 1
          ? extra - grown
          : Math.floor((extra * item.width) / natural);
      grown += share;
      const group: DiagramGroup = {
        name: item.region.name,
        x: cursorX,
        y: cursorY,
        width: item.width + share,
        height,
      };
      groups.push(group);
      const cell = Math.min((group.width - PAD_X * 2) / item.cols, MAX_CELL_W);
      const offset = (group.width - PAD_X * 2 - cell * item.cols) / 2;
      item.region.devices.forEach((device, deviceIndex) => {
        const col = deviceIndex % item.cols;
        const line = Math.floor(deviceIndex / item.cols);
        nodes.push({
          name: device.name,
          region: item.region.name,
          x: Math.round(group.x + PAD_X + offset + col * cell + (cell - NODE_W) / 2),
          y: group.y + PAD_TOP + line * NODE_H,
          kind: deviceKind({ name: device.name, type: device.type }),
          host: device.host,
          port: device.port,
          mobile: device.mobile,
          presence: device.presence,
        });
      });
      cursorX += group.width + GAP;
      maxX = Math.max(maxX, group.x + group.width);
    });
    cursorY += height + GAP;
  }
  const maxY = rows.length > 0 ? cursorY - GAP : MARGIN;

  const byName = new Map(nodes.map((node) => [node.name, node]));
  const edges: DiagramEdge[] = [];
  for (const region of view.regions) {
    for (const device of region.devices) {
      const jump = typeof device.jump === "string" ? device.jump.trim() : "";
      if (!jump) {
        continue;
      }
      const from = byName.get(jump);
      const to = byName.get(device.name);
      if (!from || !to) {
        continue;
      }
      edges.push({
        from: from.name,
        to: to.name,
        crossRegion: from.region !== to.region,
      });
    }
  }

  return {
    groups,
    nodes,
    edges,
    width: Math.max(width, maxX + MARGIN),
    height: maxY + MARGIN,
  };
}

export function renderDiagramSvg(
  model: DiagramModel,
  options?: { canOpen?: boolean },
): string {
  const byName = new Map(model.nodes.map((node) => [node.name, node]));
  const edges = model.edges
    .map((edge) => {
      const from = byName.get(edge.from);
      const to = byName.get(edge.to);
      if (!from || !to) {
        return "";
      }
      const route = routeOf(from, to);
      const [start, end] = endpoints(from, to, route);
      const path = curve(start[0], start[1], end[0], end[1], edge.crossRegion, route);
      const cls = edge.crossRegion ? "jump-edge cross-region" : "jump-edge";
      const dash = edge.crossRegion ? ' stroke-dasharray="7 5"' : "";
      return `<path class="${cls}" d="${path}" fill="none" stroke="#79a8ff" stroke-width="2.25" stroke-linecap="round"${dash} marker-end="url(#diagram-jump-arrow)"/>`;
    })
    .join("");

  const frames = model.groups
    .map((group) => {
      const title = escapeXml(group.name);
      return (
        `<g>` +
        `<rect class="region-frame" x="${group.x}" y="${group.y}" width="${group.width}" height="${group.height}" rx="14" fill="#151a21" stroke="#262d38" stroke-width="1"/>` +
        `<text class="diagram-region-title" x="${group.x + 20}" y="${group.y + 33}" fill="#e6edf3" font-size="16" font-weight="600">${title}</text>` +
        `</g>`
      );
    })
    .join("");

  const canOpen = options?.canOpen !== false;
  const nodes = model.nodes.map((node, index) => renderNode(node, index, canOpen)).join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" class="diagram" viewBox="0 0 ${model.width} ${model.height}" width="${model.width}" height="${model.height}" role="group" font-family="ui-sans-serif, system-ui, sans-serif">` +
    `<defs>${iconSymbols()}` +
    `<marker id="diagram-jump-arrow" viewBox="0 0 12 12" refX="10" refY="6" markerWidth="13" markerHeight="13" markerUnits="userSpaceOnUse" orient="auto">` +
    `<path class="jump-arrow" d="M1 1.5 11 6 1 10.5 3.5 6z" fill="#79a8ff"/>` +
    `</marker></defs>` +
    frames +
    `<g class="diagram-edges">${edges}</g>` +
    nodes +
    `</svg>`
  );
}

export function bindDeviceActivation(
  target: EventTarget,
  name: string,
  onConnect: (name: string) => void,
): void {
  target.addEventListener("click", () => {
    onConnect(name);
  });
  target.addEventListener("keydown", (event) => {
    if (!hasKey(event) || (event.key !== "Enter" && event.key !== " ")) {
      return;
    }
    event.preventDefault();
    onConnect(name);
  });
}

function renderNode(node: DiagramNode, index: number, canOpen: boolean): string {
  const name = escapeXml(node.name);
  const address = `${escapeXml(node.host)}:${node.port}`;
  const label = `连接 ${name}`;
  const title = `${name} ${address}${node.mobile ? " 移动设备" : ""}`;
  const shownTitle = canOpen ? title : `${title} ${OPEN_LIMIT_HINT}`;
  const disabled = canOpen ? "" : ' aria-disabled="true"';
  const tileX = node.x + TILE_X;
  const tileY = node.y + TILE_Y;
  const iconX = tileX + (TILE - ICON) / 2;
  const iconY = tileY + (TILE - ICON) / 2;
  const center = node.x + NODE_W / 2;
  const presence = presenceMark(node);
  const badge = node.mobile ? mobileBadge(tileX + TILE - 11, tileY - 7) : "";
  return (
    `<g class="device" role="button" tabindex="0" data-kind="${node.kind}" data-presence="${node.presence}" data-device-name="${name}" aria-label="${label}"${disabled}>` +
    `<title>${shownTitle}</title>` +
    `<rect class="diagram-node-hit" x="${node.x + 4}" y="${node.y + 2}" width="${NODE_W - 8}" height="${NODE_H - 8}" rx="12" fill="transparent" stroke="transparent" stroke-width="1.5"/>` +
    `<rect class="diagram-tile" x="${tileX}" y="${tileY}" width="${TILE}" height="${TILE}" rx="16" fill="#232a35" stroke="#354050" stroke-width="1.5"/>` +
    `<use class="diagram-glyph" href="#diagram-icon-${node.kind}" x="${iconX}" y="${iconY}" width="${ICON}" height="${ICON}" color="#d8e1ec"/>` +
    presence +
    badge +
    `<text class="diagram-name" x="${center}" y="${node.y + NAME_Y}" text-anchor="middle" fill="#e6edf3" font-size="14" font-weight="500" clip-path="url(#diagram-clip-${index})">${name}</text>` +
    `<text class="diagram-address" x="${center}" y="${node.y + ADDRESS_Y}" text-anchor="middle" fill="#9ba7b6" font-size="12" clip-path="url(#diagram-clip-${index})">${address}</text>` +
    `<clipPath id="diagram-clip-${index}"><rect x="${node.x + 6}" y="${node.y + TILE_Y + TILE + 2}" width="${NODE_W - 12}" height="${LABEL_BOTTOM - TILE_Y - TILE}"/></clipPath>` +
    `</g>`
  );
}

function presenceMark(node: DiagramNode): string {
  if (node.presence === "none") {
    return "";
  }
  const cx = node.x + TILE_X + 4;
  const cy = node.y + TILE_Y + TILE - 4;
  if (node.presence === "unknown") {
    return `<circle class="diagram-presence diagram-presence-unknown" cx="${cx}" cy="${cy}" r="5.5" fill="#151a21" stroke="#d6a64a" stroke-width="2"/>`;
  }
  const fill = node.presence === "online" ? "#3fb950" : "#6e7a8a";
  const tone = node.presence === "online" ? "online" : "offline";
  return `<circle class="diagram-presence diagram-presence-${tone}" cx="${cx}" cy="${cy}" r="6" fill="${fill}" stroke="#151a21" stroke-width="2.5"/>`;
}

function mobileBadge(x: number, y: number): string {
  return (
    `<g class="mobile-badge" transform="translate(${x} ${y})">` +
    `<circle cx="9" cy="9" r="9" fill="#12301f" stroke="#3fb950" stroke-width="1.5"/>` +
    `<rect x="5.75" y="4" width="6.5" height="10" rx="1.4" fill="none" stroke="#9be3ad" stroke-width="1.3"/>` +
    `</g>`
  );
}

function guessKind(name: string): DeviceKind {
  const tokens = tokenize(name);
  for (const [kind, words] of KEYWORDS) {
    for (const token of tokens) {
      for (const word of words) {
        if (keywordHit(token, word)) {
          return kind;
        }
      }
    }
  }
  return "generic";
}

function tokenize(name: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let mode: "ascii" | "cjk" | null = null;
  const flush = () => {
    if (current) {
      tokens.push(current);
    }
    current = "";
    mode = null;
  };
  for (const char of name.toLowerCase()) {
    const next = isAsciiWord(char) ? "ascii" : isCjk(char) ? "cjk" : null;
    if (next === null) {
      flush();
      continue;
    }
    if (mode !== null && mode !== next) {
      flush();
    }
    current += char;
    mode = next;
  }
  flush();
  return tokens;
}

function keywordHit(token: string, keyword: string): boolean {
  if (isCjk(keyword)) {
    return token.includes(keyword);
  }
  return token === keyword;
}

function isNamedKind(value: string): value is DeviceKind {
  return NAMED_KINDS.includes(value as DeviceKind);
}

function isAsciiWord(char: string): boolean {
  return (char >= "a" && char <= "z") || (char >= "0" && char <= "9");
}

function isCjk(char: string): boolean {
  const code = char.codePointAt(0) ?? 0;
  return (code >= 0x3400 && code <= 0x9fff) || (code >= 0xf900 && code <= 0xfaff);
}

function hasKey(event: Event): event is Event & { key: string } {
  return "key" in event && typeof event.key === "string";
}

function tileCenter(node: DiagramNode): [number, number] {
  return [node.x + TILE_X + TILE / 2, node.y + TILE_Y + TILE / 2];
}

/**
 * Edges leave and enter at the icon tiles. A mostly vertical edge leaves a
 * lower node above its tile and an upper node below its labels, so the line
 * never runs through the name or address.
 */
function endpoints(
  from: DiagramNode,
  to: DiagramNode,
  route: EdgeRoute,
): [[number, number], [number, number]] {
  const [x1, y1] = tileCenter(from);
  const [x2, y2] = tileCenter(to);
  if (route === "over") {
    return [
      [x1, y1 - TILE / 2 - 4],
      [x2, y2 - TILE / 2 - 6],
    ];
  }
  if (route === "vertical") {
    const below = LABEL_BOTTOM - TILE_Y - TILE / 2;
    if (y2 > y1) {
      return [
        [x1, y1 + below + 4],
        [x2, y2 - TILE / 2 - 6],
      ];
    }
    return [
      [x1, y1 - TILE / 2 - 4],
      [x2, y2 + below + 6],
    ];
  }
  return [exitTile(x1, y1, x2, y2, 6), exitTile(x2, y2, x1, y1, 8)];
}

/** Point where the ray from the tile center toward (x2, y2) leaves the tile, plus a gap. */
function exitTile(x1: number, y1: number, x2: number, y2: number, gap: number): [number, number] {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.hypot(dx, dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const half = TILE / 2;
  const toEdge = Math.min(
    ux === 0 ? Infinity : half / Math.abs(ux),
    uy === 0 ? Infinity : half / Math.abs(uy),
  );
  const distance = toEdge + gap;
  return [x1 + ux * distance, y1 + uy * distance];
}

/**
 * vertical: different rows. over: same row but not neighbours, so the edge
 * arcs above the icons instead of crossing the nodes in between. arc: neighbours.
 */
type EdgeRoute = "vertical" | "over" | "arc";

function routeOf(from: DiagramNode, to: DiagramNode): EdgeRoute {
  const [x1, y1] = tileCenter(from);
  const [x2, y2] = tileCenter(to);
  if (Math.abs(y2 - y1) > NODE_H / 2) {
    return "vertical";
  }
  return Math.abs(x2 - x1) > NODE_W * 1.5 ? "over" : "arc";
}

function curve(
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  cross: boolean,
  route: EdgeRoute,
): string {
  const dx = x2 - x1;
  const dy = y2 - y1;
  if (route === "over") {
    // Peak stays below the region titles.
    const top = Math.round(Math.min(y1, y2) - 22);
    return `M ${Math.round(x1)} ${Math.round(y1)} C ${Math.round(x1)} ${top}, ${Math.round(x2)} ${top}, ${Math.round(x2)} ${Math.round(y2)}`;
  }
  if (route === "vertical") {
    const midY = Math.round((y1 + y2) / 2);
    return `M ${Math.round(x1)} ${Math.round(y1)} C ${Math.round(x1)} ${midY}, ${Math.round(x2)} ${midY}, ${Math.round(x2)} ${Math.round(y2)}`;
  }
  const len = Math.hypot(dx, dy) || 1;
  const bend = cross ? 36 : 18;
  let px = (-dy / len) * bend;
  let py = (dx / len) * bend;
  if (py > 0) {
    px = -px;
    py = -py;
  }
  const c1x = Math.round(x1 + dx / 3 + px);
  const c1y = Math.round(y1 + dy / 3 + py);
  const c2x = Math.round(x1 + (dx * 2) / 3 + px);
  const c2y = Math.round(y1 + (dy * 2) / 3 + py);
  return `M ${Math.round(x1)} ${Math.round(y1)} C ${c1x} ${c1y}, ${c2x} ${c2y}, ${Math.round(x2)} ${Math.round(y2)}`;
}

function escapeXml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}
