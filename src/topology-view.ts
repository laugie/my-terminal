import {
  connectionChoices,
  menuAction,
  relayCommandError,
  renderConnectionMenu,
} from "./connection-menu.ts";
import { bindDeviceActivation, buildDiagram, renderDiagramSvg } from "./diagram.ts";
import { sessionLimitMessage } from "./tabs.ts";
import type { TopologyView } from "./topology.ts";

const OPEN_LIMIT_HINT = sessionLimitMessage(8);

let closeMenu: (() => void) | null = null;

export function renderTopology(
  root: HTMLElement,
  view: TopologyView,
  actions: {
    onConnect(name: string, connectionName?: string | null): void;
    activeName: string | null;
    canOpen: boolean;
  },
): void {
  closeMenu?.();
  closeMenu = null;
  root.replaceChildren();
  if (view.state === "missing" || view.state === "error") {
    const hint = el("p", "topology-hint");
    hint.setAttribute("role", "alert");
    hint.textContent = view.message ?? "无法读取拓扑配置";
    root.append(hint);
    return;
  }
  if (view.state === "empty" || view.regions.every((region) => region.devices.length === 0)) {
    const hint = el("p", "topology-hint");
    hint.textContent = view.message ?? "没有设备";
    root.append(hint);
    return;
  }

  const scroll = el("div", "diagram-scroll");
  scroll.innerHTML = renderDiagramSvg(buildDiagram(view), { canOpen: actions.canOpen });
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
      const names = connectionsOf(view, deviceName);
      if (!connectionChoices(names)) {
        actions.onConnect(deviceName);
        return;
      }
      showConnectionMenu(root, deviceName, names ?? [], actions.onConnect);
    });
  }
  syncDeviceOpenState(scroll, actions.canOpen);
  root.append(scroll);
}

export function syncDeviceOpenState(root: ParentNode, canOpen: boolean): void {
  for (const node of root.querySelectorAll<SVGElement>(".device")) {
    if (!node.dataset.openTitle) {
      node.dataset.openTitle = node.querySelector("title")?.textContent ?? "";
    }
    const title = node.querySelector("title");
    const base = node.dataset.openTitle ?? "";
    node.classList.toggle("device-open-disabled", !canOpen);
    if (canOpen) {
      node.removeAttribute("aria-disabled");
      if (title) {
        title.textContent = base;
      }
      continue;
    }
    node.setAttribute("aria-disabled", "true");
    if (title) {
      title.textContent = base.includes(OPEN_LIMIT_HINT) ? base : `${base} ${OPEN_LIMIT_HINT}`.trim();
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
