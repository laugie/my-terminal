import {
  connectionChoices,
  menuAction,
  relayCommandError,
  renderConnectionMenu,
} from "./connection-menu.ts";
import { bindDeviceActivation, buildDiagram, renderDiagramSvg } from "./diagram.ts";
import { sessionLimitMessage } from "./tabs.ts";
import type { TopologyView } from "./topology.ts";
import type { TopologyMode } from "./appearance.ts";
import type { PlanetView } from "./planet-view.ts";

const OPEN_LIMIT_HINT = sessionLimitMessage(8);

let closeMenu: (() => void) | null = null;


export interface TopologyActions {
  onConnect(name: string, connectionName?: string | null): void;
  activeName: string | null;
  canOpen: boolean;
  /** Available width in CSS pixels; the diagram is laid out to fill it. */
  width?: number;
  /** 3D planets (default) or the flat diagram. */
  mode?: TopologyMode;
}

/** The mounted 3D view. Kept across repaints so the camera does not jump. */
let planets: { root: HTMLElement; view: PlanetView } | null = null;
let planetLoad = 0;
let webgl: boolean | null = null;

export function renderTopology(root: HTMLElement, view: TopologyView, actions: TopologyActions): void {
  closeMenu?.();
  closeMenu = null;
  const empty =
    view.state === "missing" ||
    view.state === "error" ||
    view.state === "empty" ||
    view.regions.every((region) => region.devices.length === 0);
  if (empty || actions.mode === "2d") {
    disposePlanets();
  }
  if (!empty && actions.mode !== "2d") {
    renderPlanets(root, view, actions);
    return;
  }
  root.classList.remove("planet-mode");
  root.replaceChildren();
  if (view.state === "missing" || view.state === "error") {
    const hint = el("p", "topology-hint");
    hint.setAttribute("role", "alert");
    hint.textContent = view.message ?? "无法读取拓扑配置";
    root.append(hint);
    return;
  }
  if (empty) {
    const hint = el("p", "topology-hint");
    hint.textContent = view.message ?? "没有设备";
    root.append(hint);
    return;
  }
  renderDiagram(root, view, actions);
}

function renderDiagram(root: HTMLElement, view: TopologyView, actions: TopologyActions): void {
  const scroll = el("div", "diagram-scroll");
  scroll.innerHTML = renderDiagramSvg(buildDiagram(view, { width: actions.width }), { canOpen: actions.canOpen });
  for (const node of scroll.querySelectorAll<SVGElement>(".device")) {
    const name = node.dataset.deviceName ?? "";
    if (name === actions.activeName) {
      node.classList.add("device-active");
    }
    if (!actions.canOpen && !node.dataset.openTitle) {
      const text = node.querySelector("title")?.textContent ?? "";
      const suffix = ` ${OPEN_LIMIT_HINT}`;
      if (text.endsWith(suffix)) {
        node.dataset.openTitle = text.slice(0, -suffix.length);
      }
    }
    bindDeviceActivation(node, name, (deviceName) => {
      activate(root, view, deviceName, actions.onConnect);
    });
  }
  syncDeviceOpenState(scroll, actions.canOpen);
  root.append(scroll);
}

function renderPlanets(root: HTMLElement, view: TopologyView, actions: TopologyActions): void {
  const options = {
    onActivate: (deviceName: string) => {
      activate(root, view, deviceName, actions.onConnect);
    },
  };
  const mounted = planets;
  if (mounted && mounted.root === root && root.contains(mounted.view.element)) {
    mounted.view.update(view, options);
    markPlanets(root, actions);
    return;
  }
  disposePlanets();
  const load = ++planetLoad;
  if (webgl === null) {
    // Probe once; a context per repaint would hit the browser's limit.
    webgl = probeWebgl();
  }
  if (!webgl) {
    renderFallback(root, view, actions, "这台电脑的 webview 不支持 WebGL，已改用平面图。");
    return;
  }
  root.classList.add("planet-mode");
  root.replaceChildren();
  const loading = el("p", "planet-loading");
  loading.textContent = "正在加载 3D 视图…";
  root.append(loading);
  void import("./planet-view.ts")
    .then((module) => {
      if (load !== planetLoad) {
        return;
      }
      loading.remove();
      planets = { root, view: module.mountPlanetView(root, view, options) };
      markPlanets(root, actions);
    })
    .catch(() => {
      if (load === planetLoad) {
        renderFallback(root, view, actions, "3D 视图加载失败，已改用平面图。");
      }
    });
}

function renderFallback(root: HTMLElement, view: TopologyView, actions: TopologyActions, reason: string): void {
  disposePlanets();
  root.classList.remove("planet-mode");
  root.replaceChildren();
  const note = el("p", "planet-fallback");
  note.textContent = reason;
  root.append(note);
  renderDiagram(root, view, actions);
}

function markPlanets(root: HTMLElement, actions: TopologyActions): void {
  for (const node of root.querySelectorAll<HTMLElement>(".planet-label")) {
    node.classList.toggle("device-active", node.dataset.deviceName === actions.activeName);
  }
  syncDeviceOpenState(root, actions.canOpen);
}

function disposePlanets(): void {
  planetLoad += 1;
  planets?.view.dispose();
  planets = null;
}

function probeWebgl(): boolean {
  try {
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("webgl2") ?? canvas.getContext("webgl");
    context?.getExtension("WEBGL_lose_context")?.loseContext();
    return context !== null;
  } catch {
    return false;
  }
}

/** One device chosen in either view: connect, or ask which connection. */
function activate(
  root: HTMLElement,
  view: TopologyView,
  deviceName: string,
  onConnect: (name: string, connectionName?: string | null) => void,
): void {
  const names = connectionsOf(view, deviceName);
  if (!connectionChoices(names)) {
    onConnect(deviceName);
    return;
  }
  showConnectionMenu(root, deviceName, names ?? [], onConnect);
}

/**
 * Mark every device button as able to open a tab or not. SVG nodes keep the
 * hint in a <title> child; the 3D labels use the title attribute.
 */
export function syncDeviceOpenState(root: ParentNode, canOpen: boolean): void {
  for (const node of root.querySelectorAll<HTMLElement | SVGElement>(".device")) {
    const title = node.querySelector("title");
    if (!node.dataset.openTitle) {
      node.dataset.openTitle = title ? title.textContent ?? "" : node.getAttribute("title") ?? "";
    }
    const base = node.dataset.openTitle ?? "";
    const shown = canOpen
      ? base
      : base.includes(OPEN_LIMIT_HINT)
        ? base
        : `${base} ${OPEN_LIMIT_HINT}`.trim();
    node.classList.toggle("device-open-disabled", !canOpen);
    if (canOpen) {
      node.removeAttribute("aria-disabled");
    } else {
      node.setAttribute("aria-disabled", "true");
    }
    if (title) {
      title.textContent = shown;
    } else {
      node.setAttribute("title", shown);
    }
  }
}

function connectionsOf(view: TopologyView, name: string): string[] | undefined {
  for (const region of view.regions) {
    for (const device of region.devices) {
      if (device.name === name) {
        return device.connections;
      }
    }
  }
  return undefined;
}

function showConnectionMenu(
  root: HTMLElement,
  deviceName: string,
  names: readonly string[],
  onConnect: (name: string, connectionName?: string | null) => void,
): void {
  closeMenu?.();
  const host = document.createElement("div");
  host.className = "connection-menu-host";
  host.innerHTML = renderConnectionMenu(deviceName, names);
  const menu = host.querySelector<HTMLElement>(".connection-menu");
  const items = [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
  let index = 0;

  const focusAt = (next: number) => {
    index = next;
    items[index]?.focus();
  };

  const close = () => {
    document.removeEventListener("keydown", onKey, true);
    document.removeEventListener("mousedown", onPointer, true);
    host.remove();
    if (closeMenu === close) {
      closeMenu = null;
    }
  };
  closeMenu = close;

  items.forEach((button, itemIndex) => {
    button.addEventListener("click", () => {
      const raw = button.dataset.connection ?? "";
      close();
      onConnect(deviceName, raw === "" ? null : raw);
    });
    button.addEventListener("focus", () => {
      index = itemIndex;
    });
  });

  const input = host.querySelector<HTMLInputElement>(".relay-command-input");
  const error = host.querySelector(".relay-command-error");
  input?.addEventListener("input", () => {
    if (error) {
      error.textContent = relayCommandError(input.value) ?? "";
    }
  });

  menu?.addEventListener("keydown", (event) => {
    const target = event.target;
    if (target instanceof HTMLElement && target.classList.contains("relay-command-input")) {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
      return;
    }
    const action = menuAction(event.key, items.length, index);
    if (action.type === "ignore") {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (action.type === "move") {
      focusAt(action.index);
      return;
    }
    if (action.type === "close") {
      close();
      return;
    }
    items[index]?.click();
  });

  function onKey(event: KeyboardEvent): void {
    if (event.key !== "Escape") {
      return;
    }
    event.preventDefault();
    close();
  }

  function onPointer(event: MouseEvent): void {
    const target = event.target;
    if (target instanceof Node && host.contains(target)) {
      return;
    }
    close();
  }

  document.addEventListener("keydown", onKey, true);
  document.addEventListener("mousedown", onPointer, true);
  root.prepend(host);
  focusAt(0);
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
