import { TAB_LIMIT_MESSAGE } from "./terminal.ts";
import type { TopologyDevice, TopologyView } from "./topology.ts";

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

  for (const region of view.regions) {
    const section = el("section", "region");
    const title = el("h2", "region-title");
    title.textContent = region.name;
    section.append(title);
    if (region.devices.length === 0) {
      const empty = el("p", "empty");
      empty.textContent = "没有设备";
      section.append(empty);
    } else {
      const list = el("ul", "devices");
      for (const device of region.devices) {
        list.append(renderDevice(device, actions));
      }
      section.append(list);
    }
    root.append(section);
  }
}

function renderDevice(
  device: TopologyDevice,
  actions: {
    onConnect(name: string): void;
    activeName: string | null;
    canOpen: boolean;
  },
): HTMLLIElement {
  const item = el("li", "device");
  item.dataset.deviceName = device.name;
  if (device.name === actions.activeName) {
    item.classList.add("device-active");
  }
  if (!actions.canOpen) {
    item.classList.add("device-open-disabled");
    item.title = TAB_LIMIT_MESSAGE;
  }
  item.addEventListener("click", () => {
    void actions.onConnect(device.name);
  });

  const main = el("div", "host-main");
  if (device.presence !== "none") {
    const dot = el("span", dotClass(device.presence));
    dot.setAttribute("aria-label", presenceLabel(device.presence));
    main.append(dot);
  }
  const name = el("span", "host-name");
  name.textContent = device.name;
  main.append(name);
  if (device.mobile) {
    const tag = el("span", "tag mobile");
    tag.textContent = "移动设备";
    main.append(tag);
  }

  const meta = el("div", "host-meta");
  const address = el("span", "host-address");
  address.textContent = `${device.user}@${device.host}:${device.port}`;
  meta.append(address);
  if (device.presence !== "none") {
    const presence = el("span", "tag");
    presence.textContent = presenceLabel(device.presence);
    meta.append(presence);
  }
  const connect = document.createElement("button");
  connect.type = "button";
  connect.className = "button";
  connect.textContent = "连接";
  meta.append(connect);
  item.append(main, meta);
  return item;
}

function presenceLabel(presence: TopologyDevice["presence"]): string {
  switch (presence) {
    case "online":
      return "在线";
    case "offline":
      return "离线";
    case "unknown":
      return "未知";
    case "none":
      return "";
  }
}

function dotClass(presence: TopologyDevice["presence"]): string {
  switch (presence) {
    case "online":
      return "dot online";
    case "offline":
      return "dot offline";
    case "unknown":
      return "dot unknown";
    case "none":
      return "dot";
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
