import { statusLabel, type TabModel, type TabStrip } from "./tabs.ts";
import { uiIcon, type UiIconName } from "./ui-icons.ts";

export interface TabBar {
  render(strip: TabStrip): void;
  setAlert(message: string): void;
  end: HTMLElement;
}

export function mountTabBar(
  parent: HTMLElement,
  handlers: {
    onActivate(key: string): void;
    onClose(key: string): void;
    onNew(): void;
    onReconnect(): void;
  },
): TabBar {
  const bar = el("div", "tabbar");
  const list = el("div", "tabbar-list tab-list");
  list.setAttribute("role", "tablist");
  list.setAttribute("aria-label", "标签");

  const fresh = toolButton("plus", "新建", { iconOnly: true });
  fresh.classList.add("tabbar-new");
  fresh.title = "新建（回到拓扑首页）";
  fresh.addEventListener("click", () => {
    handlers.onNew();
  });

  const reconnect = toolButton("reconnect", "重新连接");
  reconnect.classList.add("tabbar-reconnect", "attention");
  reconnect.hidden = true;
  reconnect.addEventListener("click", () => {
    handlers.onReconnect();
  });

  const alert = el("p", "tabbar-alert");
  alert.setAttribute("role", "alert");
  alert.hidden = true;

  const end = el("div", "tabbar-end");
  end.append(reconnect);
  bar.append(list, fresh, end);
  parent.prepend(bar, alert);

  function render(strip: TabStrip): void {
    list.replaceChildren();
    for (const tab of strip.tabs) {
      list.append(renderTab(tab, tab.key === strip.activeKey));
    }
    const active = strip.tabs.find((tab) => tab.key === strip.activeKey);
    reconnect.hidden = !active || active.kind !== "session" || active.status !== "ended";
  }

  function renderTab(tab: TabModel, selected: boolean): HTMLElement {
    const element = el("div", "tab");
    element.setAttribute("role", "tab");
    element.tabIndex = 0;
    element.classList.toggle("tab-active", selected);
    element.classList.toggle("tab-home", tab.kind === "home");
    element.setAttribute("aria-selected", selected ? "true" : "false");
    const caption = tabCaption(tab);
    element.title = caption;
    element.setAttribute("aria-label", caption);
    if (tab.kind === "home") {
      element.insertAdjacentHTML("beforeend", uiIcon("topology"));
    }
    if (tab.kind === "session") {
      const dot = el("span", `tab-status tab-status-${tab.status}`);
      dot.setAttribute("aria-hidden", "true");
      element.append(dot);
      element.classList.toggle("tab-ended", tab.status === "ended" || tab.status === "failed");
    }
    const title = el("span", "tab-title");
    title.textContent = tab.title;
    element.append(title);
    if (tab.kind === "session") {
      const close = document.createElement("button");
      close.type = "button";
      close.className = "tab-close";
      close.setAttribute("aria-label", "关闭标签");
      close.innerHTML = uiIcon("close");
      close.addEventListener("click", (event) => {
        event.stopPropagation();
        handlers.onClose(tab.key);
      });
      element.append(close);
    }
    element.addEventListener("click", () => {
      handlers.onActivate(tab.key);
    });
    element.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") {
        return;
      }
      event.preventDefault();
      handlers.onActivate(tab.key);
    });
    return element;
  }

  return {
    render,
    end,
    setAlert(message: string): void {
      alert.textContent = message;
      alert.hidden = message === "";
    },
  };
}

/** Flat toolbar button with an icon and an optional visible label. */
export function toolButton(
  icon: UiIconName,
  label: string,
  options: { iconOnly?: boolean } = {},
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = options.iconOnly ? "tool-button icon-only" : "tool-button";
  button.setAttribute("aria-label", label);
  button.insertAdjacentHTML("beforeend", uiIcon(icon));
  if (options.iconOnly) {
    button.title = label;
  } else {
    const text = el("span", "tool-label");
    text.textContent = label;
    button.append(text);
  }
  return button;
}

function tabCaption(tab: TabModel): string {
  if (tab.kind === "home") {
    return tab.title;
  }
  const status = statusLabel(tab.status);
  if (tab.detail) {
    return `${tab.title}，${status}（${tab.detail}）`;
  }
  return `${tab.title}，${status}`;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
