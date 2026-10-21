import { iconSymbols } from "./icons.js";
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

const NODE_W = 120;
const NODE_H = 96;
const NODE_COLS = 4;
const GROUP_COLS = 3;
const PAD_X = 16;
const PAD_TOP = 40;
const PAD_BOTTOM = 16;
const GAP = 28;
const MARGIN = 56;
const CONTAINER_W = 1040;
const ICON = 48;
const ICON_X = (NODE_W - ICON) / 2;
const ICON_Y = 6;
const OPEN_LIMIT_HINT = "最多 8 个标签";

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

export function buildDiagram(view: TopologyView): DiagramModel {
  const groups: DiagramGroup[] = [];
  const nodes: DiagramNode[] = [];
  let cursorX = MARGIN;
  let cursorY = MARGIN;
  let rowHeight = 0;
  let rowCount = 0;
  let maxX = MARGIN;
  let maxY = MARGIN;

  for (const region of view.regions) {
    const size = groupSize(region.devices.length);
    const wrap =
      rowCount >= GROUP_COLS || (rowCount > 0 && cursorX + size.width > MARGIN + CONTAINER_W);
    if (wrap) {
      cursorX = MARGIN;
      cursorY += rowHeight + GAP;
      rowHeight = 0;
      rowCount = 0;
    }
    const group: DiagramGroup = {
      name: region.name,
      x: cursorX,
      y: cursorY,
      width: size.width,
      height: size.height,
    };
    groups.push(group);
    region.devices.forEach((device, index) => {
      const col = index % NODE_COLS;
      const row = Math.floor(index / NODE_COLS);
      nodes.push({
        name: device.name,
        region: region.name,
        x: group.x + PAD_X + col * NODE_W,
        y: group.y + PAD_TOP + row * NODE_H,
        kind: deviceKind({ name: device.name, type: device.type }),
        host: device.host,
        port: device.port,
        mobile: device.mobile,
        presence: device.presence,
      });
    });
    cursorX += size.width + GAP;
    rowHeight = Math.max(rowHeight, size.height);
    rowCount += 1;
    maxX = Math.max(maxX, group.x + group.width);
    maxY = Math.max(maxY, group.y + group.height);
  }

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
    width: maxX + MARGIN,
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
      const [start, end] = endpoints(from, to, edge.crossRegion);
      const path = curve(start[0], start[1], end[0], end[1], edge.crossRegion);
      const cls = edge.crossRegion ? "jump-edge cross-region" : "jump-edge";
      const dash = edge.crossRegion ? ' stroke-dasharray="6 4"' : "";
      return `<path class="${cls}" d="${path}" fill="none" stroke="#8eb4d4" stroke-width="1.6"${dash} marker-end="url(#diagram-jump-arrow)"/>`;
    })
    .join("");

  const frames = model.groups
    .map((group) => {
      const title = escapeXml(group.name);
      return (
        `<g>` +
        `<rect class="region-frame" x="${group.x}" y="${group.y}" width="${group.width}" height="${group.height}" rx="12" fill="#252526" fill-opacity="0.78" stroke="#3c3c3c" stroke-width="1.5"/>` +
        `<text class="diagram-region-title" x="${group.x + 16}" y="${group.y + 26}" fill="#e6e6e6" font-size="14" font-weight="600">${title}</text>` +
        `</g>`
      );
    })
    .join("");

  const canOpen = options?.canOpen !== false;
  const nodes = model.nodes.map((node, index) => renderNode(node, index, canOpen)).join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" class="diagram" viewBox="0 0 ${model.width} ${model.height}" width="${model.width}" height="${model.height}" role="group" font-family="ui-sans-serif, system-ui, sans-serif">` +
    `<defs>${iconSymbols()}` +
    `<marker id="diagram-jump-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto">` +
    `<path d="M1 1.2 8.2 5 1 8.8" fill="none" stroke="#8eb4d4" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>` +
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
  const iconX = node.x + ICON_X;
  const iconY = node.y + ICON_Y;
  const center = node.x + NODE_W / 2;
  const presence = presenceMark(node);
  const badge = node.mobile ? mobileBadge(iconX + ICON - 8, iconY - 2) : "";
  return (
    `<g class="device" role="button" tabindex="0" data-kind="${node.kind}" data-device-name="${name}" aria-label="${label}"${disabled}>` +
    `<title>${shownTitle}</title>` +
    `<rect class="diagram-node-hit" x="${node.x + 4}" y="${node.y + 2}" width="${NODE_W - 8}" height="${NODE_H - 4}" rx="8" fill="transparent" stroke="transparent" stroke-width="2"/>` +
    `<use href="#diagram-icon-${node.kind}" x="${iconX}" y="${iconY}" width="${ICON}" height="${ICON}" color="#d7d7d7"/>` +
    presence +
    badge +
    `<text class="diagram-name" x="${center}" y="${node.y + 70}" text-anchor="middle" fill="#e6e6e6" font-size="12" clip-path="url(#diagram-clip-${index})">${name}</text>` +
    `<text class="diagram-address" x="${center}" y="${node.y + 86}" text-anchor="middle" fill="#9a9a9a" font-size="11" clip-path="url(#diagram-clip-${index})">${address}</text>` +
    `<clipPath id="diagram-clip-${index}"><rect x="${node.x + 4}" y="${node.y + 56}" width="${NODE_W - 8}" height="36"/></clipPath>` +
    `</g>`
  );
}

function presenceMark(node: DiagramNode): string {
  if (node.presence === "none") {
    return "";
  }
  const cx = node.x + ICON_X + 8;
  const cy = node.y + ICON_Y + ICON - 8;
  if (node.presence === "unknown") {
    return `<circle class="diagram-presence diagram-presence-unknown" cx="${cx}" cy="${cy}" r="4" fill="none" stroke="#e6c98a" stroke-width="1.5"/>`;
  }
  const fill = node.presence === "online" ? "#3dd68c" : "#8a8a8a";
  const tone = node.presence === "online" ? "online" : "offline";
  return `<circle class="diagram-presence diagram-presence-${tone}" cx="${cx}" cy="${cy}" r="4" fill="${fill}"/>`;
}

function mobileBadge(x: number, y: number): string {
  return (
    `<g class="mobile-badge" transform="translate(${x} ${y})">` +
    `<circle cx="8" cy="8" r="8" fill="#163528" stroke="#3dd68c" stroke-width="1.4"/>` +
    `<rect x="5" y="3.5" width="6" height="9" rx="1.2" fill="none" stroke="#b7e4cf" stroke-width="1.2"/>` +
    `</g>`
  );
}

function groupSize(count: number): { width: number; height: number } {
  if (count <= 0) {
    return { width: PAD_X * 2 + NODE_W, height: PAD_TOP + PAD_BOTTOM };
  }
  const cols = Math.min(NODE_COLS, count);
  const rows = Math.ceil(count / NODE_COLS);
  return {
    width: PAD_X * 2 + cols * NODE_W,
    height: PAD_TOP + PAD_BOTTOM + rows * NODE_H,
  };
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

function iconCenter(node: DiagramNode): [number, number] {
  return [node.x + ICON_X + ICON / 2, node.y + ICON_Y + ICON / 2];
}

function endpoints(
  from: DiagramNode,
  to: DiagramNode,
  cross: boolean,
): [[number, number], [number, number]] {
  const [x1, y1] = iconCenter(from);
  const [x2, y2] = iconCenter(to);
  if (cross && Math.abs(y2 - y1) > 48) {
    const dir = y2 > y1 ? 1 : -1;
    return [
      [x1, y1 + dir * 28],
      [x2, y2 - dir * 34],
    ];
  }
  return [travel(x1, y1, x2, y2, 26), travel(x2, y2, x1, y1, 30)];
}

function travel(x1: number, y1: number, x2: number, y2: number, distance: number): [number, number] {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.hypot(dx, dy) || 1;
  const scale = distance / len;
  return [x1 + dx * scale, y1 + dy * scale];
}

function curve(x1: number, y1: number, x2: number, y2: number, cross: boolean): string {
  const dx = x2 - x1;
  const dy = y2 - y1;
  if (cross && Math.abs(dy) > 48) {
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
