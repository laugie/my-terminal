import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { errorText, type HostEntry } from "./hosts.ts";
import {
  closeSession,
  openSession,
  resizeSession,
  sshTargetOf,
  writeSession,
  type SessionEvent,
} from "./session.ts";

/** Matches the backend `write_session` limit. Larger pastes are split below it. */
const MAX_WRITE_BYTES = 64 * 1024;

/** Extra time after Exit before the process is reaped, so a late chunk can land. */
const EXIT_REAP_DELAY_MS = 300;

type Phase = "connecting" | "live" | "ended";

interface Tab {
  host: HostEntry;
  phase: Phase;
  sessionId: string;
  generation: number;
  pendingInput: string[];
  term: Terminal;
  fit: FitAddon;
  view: HTMLElement;
  element: HTMLElement;
  title: HTMLElement;
  endedMark: HTMLElement;
  reapTimer: number;
}

export interface TerminalPanel {
  connect(host: HostEntry): Promise<void>;
  activeHostId(): string | null;
}

export function mountTerminal(
  panel: HTMLElement,
  onChange: () => void,
  focusHostList: () => void,
): TerminalPanel {
  const bar = el("div", "terminal-bar");
  const newTab = document.createElement("button");
  newTab.type = "button";
  newTab.className = "button";
  newTab.textContent = "新建";
  newTab.addEventListener("click", () => {
    focusHostList();
  });

  const tabList = el("div", "tab-list");
  tabList.setAttribute("role", "tablist");
  tabList.setAttribute("aria-label", "会话标签");

  const reconnect = document.createElement("button");
  reconnect.type = "button";
  reconnect.className = "button";
  reconnect.textContent = "重新连接";
  reconnect.hidden = true;
  bar.append(newTab, tabList, reconnect);

  const stack = el("div", "terminal-stack");
  const empty = el("p", "terminal-empty");
  empty.textContent = "未打开会话";
  stack.append(empty);

  const sessionError = el("p", "form-error");
  sessionError.setAttribute("role", "alert");
  panel.append(bar, stack, sessionError);

  const tabs: Tab[] = [];
  let active: Tab | null = null;
  let resizeFrame = 0;
  let suppressPaste = false;

  reconnect.addEventListener("click", () => {
    if (active && active.phase === "ended") {
      void reconnectTab(active);
    }
  });

  const observer = new ResizeObserver(() => {
    scheduleFit();
  });
  observer.observe(stack);
  window.addEventListener("keydown", onWindowKeyDown, true);
  refreshChrome();

  async function connect(host: HostEntry): Promise<void> {
    sessionError.textContent = "";
    const tab = createTab(host);
    activate(tab);
    await openOnTab(tab, false);
  }

  function createTab(host: HostEntry): Tab {
    const view = el("div", "terminal-view");
    view.hidden = true;
    stack.append(view);

    const element = el("div", "tab");
    element.setAttribute("role", "tab");
    element.tabIndex = 0;
    const endedMark = el("span", "tab-mark");
    endedMark.textContent = "○";
    endedMark.hidden = true;
    const title = el("span", "tab-title");
    const close = document.createElement("button");
    close.type = "button";
    close.className = "tab-close";
    close.setAttribute("aria-label", "关闭标签");
    close.textContent = "×";
    element.append(endedMark, title, close);
    tabList.append(element);

    const term = new Terminal({
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      fontSize: 14,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);

    const tab: Tab = {
      host,
      phase: "connecting",
      sessionId: "",
      generation: 0,
      pendingInput: [],
      term,
      fit,
      view,
      element,
      title,
      endedMark,
      reapTimer: 0,
    };
    tabs.push(tab);

    element.addEventListener("click", () => {
      activate(tab);
    });
    element.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        activate(tab);
      }
    });
    close.addEventListener("click", (event) => {
      event.stopPropagation();
      requestClose(tab);
    });
    term.onData((data) => {
      if (tab.phase === "ended") {
        return;
      }
      if (tab.sessionId === "") {
        tab.pendingInput.push(data);
        return;
      }
      const id = tab.sessionId;
      void writeSession(id, data).catch((err: unknown) => {
        if (tab.phase === "ended" || tab.sessionId !== id) {
          return;
        }
        sessionError.textContent = errorText(err);
      });
    });
    view.addEventListener("paste", (event) => {
      if (suppressPaste || active !== tab) {
        if (suppressPaste) {
          event.preventDefault();
        }
        return;
      }
      event.preventDefault();
      const text = event.clipboardData?.getData("text/plain") ?? "";
      void writeText(tab, text).catch((err: unknown) => {
        sessionError.textContent = errorText(err);
      });
    });

    // Open only while this view is the one on screen. Fit happens later, and
    // only for the active tab.
    for (const other of tabs) {
      other.view.hidden = other !== tab;
    }
    stack.classList.add("has-tabs");
    empty.hidden = true;
    term.open(view);
    refreshTitles();
    return tab;
  }

  async function openOnTab(tab: Tab, keepBuffer: boolean): Promise<void> {
    const myGeneration = ++tab.generation;
    window.clearTimeout(tab.reapTimer);
    tab.reapTimer = 0;
    tab.phase = "connecting";
    tab.sessionId = "";
    tab.pendingInput = [];
    if (!keepBuffer) {
      tab.term.reset();
    }
    refreshChrome();
    onChange();
    try {
      const opened = await openSession(sshTargetOf(tab.host), (event) => {
        if (tab.generation !== myGeneration) {
          return;
        }
        onSessionEvent(tab, event, myGeneration);
      });
      if (tab.generation !== myGeneration) {
        await closeSession(opened.id);
        return;
      }
      tab.sessionId = opened.id;
      if (phaseOf(tab) === "ended") {
        scheduleReap(tab, opened.id, myGeneration);
        return;
      }
      tab.phase = "live";
      const queued = tab.pendingInput.splice(0, tab.pendingInput.length);
      for (const data of queued) {
        if (tab.phase !== "live" || tab.generation !== myGeneration) {
          break;
        }
        await writeSession(opened.id, data);
      }
      refreshChrome();
      onChange();
      if (active === tab) {
        scheduleFit();
        tab.term.focus();
      }
    } catch (err) {
      if (tab.generation !== myGeneration) {
        return;
      }
      sessionError.textContent = errorText(err);
      tab.pendingInput = [];
      if (keepBuffer) {
        tab.phase = "ended";
        refreshChrome();
        onChange();
      } else {
        removeTab(tab);
      }
    }
  }

  function onSessionEvent(tab: Tab, event: SessionEvent, generation: number): void {
    if (event.kind === "data") {
      tab.term.write(new Uint8Array(event.bytes));
      return;
    }
    if (tab.phase === "ended") {
      return;
    }
    const codeText = event.code === null ? "未知" : String(event.code);
    tab.term.write(`\r\n[会话已结束，退出码 ${codeText}]\r\n`);
    tab.phase = "ended";
    tab.pendingInput = [];
    refreshChrome();
    onChange();
    if (tab.sessionId !== "") {
      scheduleReap(tab, tab.sessionId, generation);
    }
  }

  function scheduleReap(tab: Tab, id: string, generation: number): void {
    window.clearTimeout(tab.reapTimer);
    tab.reapTimer = window.setTimeout(() => {
      tab.reapTimer = 0;
      if (tab.generation !== generation || tab.sessionId !== id) {
        return;
      }
      void closeSession(id).catch((err: unknown) => {
        if (tab.generation !== generation) {
          return;
        }
        sessionError.textContent = errorText(err);
      });
    }, EXIT_REAP_DELAY_MS);
  }

  async function reconnectTab(tab: Tab): Promise<void> {
    if (tab.phase !== "ended") {
      return;
    }
    window.clearTimeout(tab.reapTimer);
    tab.reapTimer = 0;
    const oldId = tab.sessionId;
    if (oldId !== "") {
      try {
        await closeSession(oldId);
      } catch (err) {
        sessionError.textContent = errorText(err);
      }
    }
    if (active === tab) {
      tab.term.focus();
    }
    await openOnTab(tab, true);
  }

  function requestClose(tab: Tab): void {
    if (tab.phase !== "ended" && !window.confirm("结束该会话？")) {
      return;
    }
    const id = tab.sessionId;
    tab.generation += 1;
    window.clearTimeout(tab.reapTimer);
    removeTab(tab);
    if (id !== "") {
      void closeSession(id).catch((err: unknown) => {
        sessionError.textContent = errorText(err);
      });
    }
  }

  function removeTab(tab: Tab): void {
    const index = tabs.indexOf(tab);
    if (index < 0) {
      return;
    }
    tabs.splice(index, 1);
    tab.term.dispose();
    tab.view.remove();
    tab.element.remove();
    if (active === tab) {
      active = null;
      const next = tabs[index] ?? tabs[index - 1] ?? null;
      if (next) {
        activate(next);
        return;
      }
    }
    refreshTitles();
    refreshChrome();
    onChange();
  }

  function activate(tab: Tab): void {
    active = tab;
    for (const other of tabs) {
      const on = other === tab;
      other.view.hidden = !on;
      other.element.classList.toggle("tab-active", on);
      other.element.setAttribute("aria-selected", on ? "true" : "false");
    }
    refreshTitles();
    refreshChrome();
    onChange();
    scheduleFit();
    if (tab.phase !== "ended") {
      tab.term.focus();
    }
  }

  function refreshTitles(): void {
    const groups = new Map<string, Tab[]>();
    for (const tab of tabs) {
      const group = groups.get(tab.host.name) ?? [];
      group.push(tab);
      groups.set(tab.host.name, group);
    }
    for (const [name, group] of groups) {
      group.forEach((tab, index) => {
        const label = index === 0 ? name : `${name} (${index + 1})`;
        tab.title.textContent = label;
        tab.element.title = label;
      });
    }
  }

  function refreshChrome(): void {
    stack.classList.toggle("has-tabs", tabs.length > 0);
    empty.hidden = tabs.length > 0;
    for (const tab of tabs) {
      const ended = tab.phase === "ended";
      tab.element.classList.toggle("tab-ended", ended);
      tab.endedMark.hidden = !ended;
    }
    reconnect.hidden = active?.phase !== "ended";
  }

  function scheduleFit(): void {
    if (resizeFrame !== 0) {
      return;
    }
    resizeFrame = window.requestAnimationFrame(() => {
      resizeFrame = 0;
      if (active) {
        applyFit(active);
      }
    });
  }

  function applyFit(tab: Tab): void {
    if (tab !== active || tab.view.hidden) {
      return;
    }
    tab.fit.fit();
    if (tab.phase !== "live" || tab.sessionId === "") {
      return;
    }
    const cols = tab.term.cols;
    const rows = tab.term.rows;
    if (cols < 1 || rows < 1) {
      return;
    }
    const id = tab.sessionId;
    void resizeSession(id, cols, rows).catch((err: unknown) => {
      if (tab.phase !== "live" || tab.sessionId !== id) {
        return;
      }
      sessionError.textContent = errorText(err);
    });
  }

  function onWindowKeyDown(event: KeyboardEvent): void {
    if (event.altKey || typingInForm(event)) {
      return;
    }
    if (event.ctrlKey && !event.metaKey && event.key === "Tab") {
      consume(event);
      cycleTab(event.shiftKey ? -1 : 1);
      return;
    }

    const mac = isMacPlatform();
    if (mac) {
      if (!event.metaKey || event.ctrlKey) {
        return;
      }
      handleCommandShortcut(event);
      return;
    }

    // Letter shortcuts need Shift. Bare Ctrl+C / Ctrl+W must reach the shell.
    if (!event.ctrlKey || event.metaKey) {
      return;
    }
    if (/^[1-8]$/.test(event.key)) {
      consume(event);
      jumpTab(Number(event.key));
      return;
    }
    if (!event.shiftKey) {
      return;
    }
    handleCommandShortcut(event);
  }

  function handleCommandShortcut(event: KeyboardEvent): void {
    if (/^[1-8]$/.test(event.key)) {
      consume(event);
      jumpTab(Number(event.key));
      return;
    }
    switch (event.key.toUpperCase()) {
      case "T":
        consume(event);
        focusHostList();
        break;
      case "W":
        consume(event);
        if (active) {
          requestClose(active);
        }
        break;
      case "C":
        consume(event);
        void copySelection().catch((err: unknown) => {
          sessionError.textContent = errorText(err);
        });
        break;
      case "V":
        consume(event);
        if (active) {
          const tab = active;
          suppressPaste = true;
          void pasteClipboard(tab)
            .catch((err: unknown) => {
              sessionError.textContent = errorText(err);
            })
            .finally(() => {
              suppressPaste = false;
            });
        }
        break;
      default:
        break;
    }
  }

  async function copySelection(): Promise<void> {
    if (!active) {
      return;
    }
    const text = active.term.getSelection();
    if (text === "") {
      return;
    }
    if (!navigator.clipboard?.writeText) {
      throw new Error("无法写入剪贴板");
    }
    await navigator.clipboard.writeText(text);
  }

  async function pasteClipboard(tab: Tab): Promise<void> {
    if (!navigator.clipboard?.readText) {
      throw new Error("无法读取剪贴板");
    }
    const text = await navigator.clipboard.readText();
    await writeText(tab, text);
  }

  async function writeText(tab: Tab, text: string): Promise<void> {
    if (phaseOf(tab) === "ended" || text === "") {
      return;
    }
    for (const chunk of splitWriteChunks(text)) {
      if (phaseOf(tab) === "ended") {
        return;
      }
      if (tab.sessionId === "") {
        tab.pendingInput.push(chunk);
        continue;
      }
      const id = tab.sessionId;
      await writeSession(id, chunk);
    }
  }

  function cycleTab(step: number): void {
    if (!active || tabs.length === 0) {
      return;
    }
    const index = tabs.indexOf(active);
    const next = tabs[(index + step + tabs.length) % tabs.length];
    if (next) {
      activate(next);
    }
  }

  function jumpTab(number: number): void {
    const tab = tabs[number - 1];
    if (tab) {
      activate(tab);
    }
  }

  return {
    connect,
    activeHostId: () => active?.host.id ?? null,
  };
}

function phaseOf(tab: Tab): Phase {
  return tab.phase;
}

function splitWriteChunks(text: string): string[] {
  const encoded = new TextEncoder().encode(text);
  if (encoded.length <= MAX_WRITE_BYTES) {
    return [text];
  }
  const chunks: string[] = [];
  const decoder = new TextDecoder();
  let offset = 0;
  while (offset < encoded.length) {
    let end = Math.min(offset + MAX_WRITE_BYTES, encoded.length);
    while (end > offset && (encoded[end] & 0xc0) === 0x80) {
      end -= 1;
    }
    if (end === offset) {
      end = Math.min(offset + MAX_WRITE_BYTES, encoded.length);
    }
    chunks.push(decoder.decode(encoded.subarray(offset, end)));
    offset = end;
  }
  return chunks;
}

function isMacPlatform(): boolean {
  const withData = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = withData.userAgentData?.platform ?? navigator.platform;
  return /mac/i.test(platform);
}

function typingInForm(event: KeyboardEvent): boolean {
  const target = event.target;
  if (!(target instanceof HTMLElement) || target.closest(".terminal-panel")) {
    return false;
  }
  return target.matches("input, textarea, select, [contenteditable='true']");
}

function consume(event: KeyboardEvent): void {
  event.preventDefault();
  event.stopPropagation();
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
