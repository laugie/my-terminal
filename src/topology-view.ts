import { bindDeviceActivation, buildDiagram, renderDiagramSvg } from "./diagram.ts";
import { sessionLimitMessage } from "./tabs.ts";
import type { TopologyView } from "./topology.ts";

const OPEN_LIMIT_HINT = sessionLimitMessage(8);

export function renderTopology(
  root: HTMLElement,
  view: TopologyView,
  actions: {
    onConnect(name: string): void;
    activeName: string | null;
    canOpen: boolean;
  },
): void {
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
      actions.onConnect(deviceName);
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

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
