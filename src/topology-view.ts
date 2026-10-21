import { bindDeviceActivation, buildDiagram, renderDiagramSvg } from "./diagram.ts";
import type { TopologyView } from "./topology.ts";

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
  scroll.innerHTML = renderDiagramSvg(buildDiagram(view));
  for (const node of scroll.querySelectorAll<SVGElement>(".device")) {
    const name = node.dataset.deviceName ?? "";
    if (name === actions.activeName) {
      node.classList.add("device-active");
    }
    if (!actions.canOpen) {
      node.classList.add("device-open-disabled");
    }
    bindDeviceActivation(node, name, (deviceName) => {
      actions.onConnect(deviceName);
    });
  }
  root.append(scroll);
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
