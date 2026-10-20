import {
  addManualHost,
  errorText,
  listHosts,
  removeManualHost,
  type HostEntry,
  type ListHostsResponse,
} from "./hosts.ts";
import { loadTopology, type TopologyView } from "./topology.ts";
import { renderTopology } from "./topology-view.ts";
import { TAB_LIMIT_MESSAGE, mountTerminal } from "./terminal.ts";
import { mountLogin } from "./login.ts";

const app = document.querySelector("#app");
if (!(app instanceof HTMLElement)) {
  throw new Error("missing #app");
}

type Page = "topology" | "manual";

const header = el("header", "top");
const title = el("h1", "title");
title.textContent = "my-terminal";
const nav = el("nav", "nav");
nav.setAttribute("aria-label", "页面");
const topologyButton = navButton("拓扑");
const manualButton = navButton("手动服务器");
nav.append(topologyButton, manualButton);
const refreshButton = document.createElement("button");
refreshButton.type = "button";
refreshButton.className = "button";
refreshButton.textContent = "重新加载";
header.append(title, nav, refreshButton);

const statusBar = el("div", "status");
statusBar.setAttribute("role", "status");

const topologyRoot = el("div", "topology");
topologyRoot.tabIndex = -1;

const manualRoot = el("div", "manual-page");
manualRoot.hidden = true;
const list = document.createElement("ul");
list.className = "hosts";

const formSection = el("section", "panel");
const formTitle = el("h2", "section-title");
formTitle.textContent = "添加服务器";
const form = document.createElement("form");
form.className = "form";
const nameInput = field(form, "名称", "name", "text", true);
const hostInput = field(form, "主机", "host", "text", true);
const userInput = field(form, "用户", "user", "text", false);
const portInput = field(form, "端口", "port", "number", false);
portInput.placeholder = "22";
portInput.min = "1";
portInput.max = "65535";
const submit = document.createElement("button");
submit.type = "submit";
submit.className = "button primary";
submit.textContent = "添加";
form.append(submit);
const formError = el("p", "form-error");
formError.setAttribute("role", "alert");
formSection.append(formTitle, form, formError);
manualRoot.append(list, formSection);

const terminalPanel = el("section", "terminal-panel");
const shell = el("div", "shell");
shell.hidden = true;
shell.append(header, statusBar, topologyRoot, manualRoot, terminalPanel);
app.append(shell);

const terminal = mountTerminal(
  terminalPanel,
  () => {
    markActive();
  },
  () => {
    const target = page === "topology" ? topologyRoot : list;
    target.tabIndex = -1;
    target.focus();
  },
);

let page: Page = "topology";
let signedIn = false;
let refreshGeneration = 0;
let topologyGeneration = 0;
let topologyBusy = false;
let hostsBusy = false;
let manualLoaded = false;
let topologyView: TopologyView | null = null;

topologyButton.addEventListener("click", () => {
  showPage("topology");
});
manualButton.addEventListener("click", () => {
  showPage("manual");
  if (!manualLoaded) {
    void refreshHosts();
  }
});
refreshButton.addEventListener("click", () => {
  if (page === "topology") {
    void refreshTopology();
  } else {
    void refreshHosts();
  }
});
form.addEventListener("submit", (event) => {
  event.preventDefault();
  void onAdd();
});

mountLogin(app, () => {
  signedIn = true;
  shell.hidden = false;
  showPage("topology");
  void refreshTopology();
});

function showPage(next: Page): void {
  page = next;
  topologyRoot.hidden = next !== "topology";
  manualRoot.hidden = next !== "manual";
  statusBar.hidden = next !== "manual";
  topologyButton.classList.toggle("active", next === "topology");
  manualButton.classList.toggle("active", next === "manual");
  topologyButton.setAttribute("aria-current", next === "topology" ? "page" : "false");
  manualButton.setAttribute("aria-current", next === "manual" ? "page" : "false");
  syncRefreshButton();
}

function syncRefreshButton(): void {
  refreshButton.disabled = page === "topology" ? topologyBusy : hostsBusy;
}

async function refreshTopology(): Promise<void> {
  if (!signedIn) {
    return;
  }
  const generation = ++topologyGeneration;
  topologyBusy = true;
  syncRefreshButton();
  try {
    const view = await loadTopology();
    if (generation !== topologyGeneration) {
      return;
    }
    topologyView = view;
    paintTopology();
  } catch (err) {
    if (generation !== topologyGeneration) {
      return;
    }
    topologyView = {
      configPath: "",
      state: "error",
      message: errorText(err),
      regions: [],
    };
    paintTopology();
  } finally {
    if (generation === topologyGeneration) {
      topologyBusy = false;
      syncRefreshButton();
    }
  }
}

function paintTopology(): void {
  if (!topologyView) {
    return;
  }
  renderTopology(topologyRoot, topologyView, {
    onConnect: (name) => {
      void terminal.connectDevice(name);
    },
    activeName: terminal.activeDeviceName(),
    canOpen: terminal.canOpenTab(),
  });
}

async function refreshHosts(): Promise<void> {
  if (!signedIn) {
    return;
  }
  const generation = ++refreshGeneration;
  hostsBusy = true;
  syncRefreshButton();
  try {
    const result = await listHosts();
    if (generation !== refreshGeneration) {
      return;
    }
    manualLoaded = true;
    render(result);
    formError.textContent = "";
  } catch (err) {
    if (generation !== refreshGeneration) {
      return;
    }
    statusBar.dataset.state = "error";
    statusBar.title = "";
    statusBar.textContent = "读取服务器列表失败";
    formError.textContent = errorText(err);
  } finally {
    if (generation === refreshGeneration) {
      hostsBusy = false;
      syncRefreshButton();
    }
  }
}

async function onAdd(): Promise<void> {
  formError.textContent = "";
  const name = nameInput.value.trim();
  const host = hostInput.value.trim();
  const user = userInput.value.trim();
  const portRaw = portInput.value.trim();
  let port: number | null = null;
  if (portRaw !== "") {
    port = Number(portRaw);
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      formError.textContent = "端口必须在 1 到 65535 之间";
      return;
    }
  }
  submit.disabled = true;
  try {
    await addManualHost({
      name,
      host,
      user: user === "" ? null : user,
      port,
    });
    form.reset();
    await refreshHosts();
  } catch (err) {
    formError.textContent = errorText(err);
  } finally {
    submit.disabled = false;
  }
}

function render(result: ListHostsResponse): void {
  statusBar.replaceChildren();
  statusBar.dataset.state = result.status;
  statusBar.title = result.message ?? "";
  if (result.configNotice) {
    const notice = el("span", "status-notice");
    notice.textContent = result.configNotice;
    statusBar.append(notice);
  }
  if (result.message) {
    const message = el("span", "status-text");
    message.textContent = result.message;
    message.title = result.message;
    statusBar.append(message);
  }
  if (result.selfName) {
    const selfName = el("span", "status-self");
    selfName.textContent = `本机：${result.selfName}`;
    statusBar.append(selfName);
  }
  if (!result.message && !result.selfName) {
    const message = el("span", "status-text");
    message.textContent = result.status === "ok" ? "Tailscale 运行中" : "";
    statusBar.append(message);
  }

  list.replaceChildren();
  if (result.hosts.length === 0) {
    const empty = el("li", "empty");
    empty.textContent = "还没有服务器";
    list.append(empty);
    return;
  }

  for (const host of result.hosts) {
    list.append(renderHost(host));
  }
}

function markActive(): void {
  const activeId = terminal.activeHostId();
  const canOpen = terminal.canOpenTab();
  for (const item of list.querySelectorAll<HTMLLIElement>(".host")) {
    item.classList.toggle("host-active", item.dataset.hostId === activeId);
    item.classList.toggle("host-open-disabled", !canOpen);
    item.title = canOpen ? "" : TAB_LIMIT_MESSAGE;
  }
  const activeName = terminal.activeDeviceName();
  for (const item of topologyRoot.querySelectorAll<HTMLLIElement>(".device")) {
    item.classList.toggle("device-active", item.dataset.deviceName === activeName);
    item.classList.toggle("device-open-disabled", !canOpen);
    item.title = canOpen ? "" : TAB_LIMIT_MESSAGE;
  }
}

function renderHost(host: HostEntry): HTMLLIElement {
  const item = el("li", "host");
  item.dataset.hostId = host.id;
  if (host.id === terminal.activeHostId()) {
    item.classList.add("host-active");
  }
  if (!terminal.canOpenTab()) {
    item.classList.add("host-open-disabled");
    item.title = TAB_LIMIT_MESSAGE;
  }
  item.addEventListener("click", (event) => {
    const target = event.target;
    if (target instanceof HTMLElement && target.closest("[data-action='remove']")) {
      return;
    }
    void terminal.connect(host);
  });
  const main = el("div", "host-main");
  const dot = el("span", host.online === true ? "dot online" : "dot offline");
  dot.setAttribute("aria-label", host.online === true ? "在线" : "离线");
  const name = el("span", "host-name");
  name.textContent = host.name;
  main.append(dot, name);

  const meta = el("div", "host-meta");
  const address = el("span", "host-address");
  address.textContent = addressLine(host);
  const tag = el("span", "tag");
  tag.textContent = sourceLabel(host.source);
  meta.append(address, tag);

  const connect = document.createElement("button");
  connect.type = "button";
  connect.className = "button";
  connect.textContent = "连接";
  meta.append(connect);

  if (host.source === "manual" || host.source === "tailscale+manual") {
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "button ghost";
    remove.dataset.action = "remove";
    remove.textContent = "移除";
    remove.addEventListener("click", (event) => {
      event.stopPropagation();
      void onRemove(host.id);
    });
    meta.append(remove);
  }

  item.append(main, meta);
  return item;
}

async function onRemove(id: string): Promise<void> {
  formError.textContent = "";
  try {
    await removeManualHost(id);
    await refreshHosts();
  } catch (err) {
    formError.textContent = errorText(err);
  }
}

function addressLine(host: HostEntry): string {
  const target = host.ip ?? host.host;
  const withUser = host.user ? `${host.user}@${target}` : target;
  return host.port == null ? withUser : `${withUser}:${host.port}`;
}

function sourceLabel(source: HostEntry["source"]): string {
  switch (source) {
    case "tailscale":
      return "Tailscale";
    case "manual":
      return "手动";
    case "tailscale+manual":
      return "Tailscale+手动";
  }
}

function navButton(label: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "button nav-button";
  button.textContent = label;
  return button;
}

function field(
  form: HTMLFormElement,
  labelText: string,
  name: string,
  type: string,
  required: boolean,
): HTMLInputElement {
  const label = document.createElement("label");
  label.className = "field";
  const caption = el("span", "field-label");
  caption.textContent = labelText;
  const input = document.createElement("input");
  input.name = name;
  input.type = type;
  input.required = required;
  input.autocomplete = "off";
  label.append(caption, input);
  form.append(label);
  return input;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
