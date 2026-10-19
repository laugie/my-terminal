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
import { splitWriteChunks } from "./write-chunks.ts";
import { WriteQueue } from "./write-queue.ts";

/** Ended tabs count. The backend limit only covers live processes. */
const MAX_TABS = 8;

export const TAB_LIMIT_MESSAGE = "最多 8 个标签";

/** Extra time after Exit before the process is reaped, so a late chunk can land. */
const EXIT_REAP_DELAY_MS = 300;

type Phase = "connecting" | "live" | "ended";

interface Tab {
  host: HostEntry;
  phase: Phase;
  sessionId: string;
  generation: number;
  /** Each entry is one keystroke or paste, already split into write-sized chunks. */
  pendingWrites: string[][];
  queue: WriteQueue;
  error: string;
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
  canOpenTab(): boolean;
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
    if (tabs.length >= MAX_TABS) {
      sessionError.textContent = TAB_LIMIT_MESSAGE;
      return;
    }
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
      pendingWrites: [],
      queue: new WriteQueue(),
      error: "",
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
    // xterm stops the paste event and feeds the whole clipboard string through
    // onData. Chunk on this path so a menu paste stays within the write limit.
    term.onData((data) => {
      submitInput(tab, data);
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
    tab.queue.invalidate();
    window.clearTimeout(tab.reapTimer);
    tab.reapTimer = 0;
    tab.phase = "connecting";
    tab.sessionId = "";
    tab.pendingWrites = [];
    clearTabError(tab);
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
        tab.pendingWrites = [];
        tab.queue.invalidate();
        scheduleReap(tab, opened.id, myGeneration);
        return;
      }
      tab.phase = "live";
      // Flush before any later await so newer onData cannot jump the queue.
      const queued = tab.pendingWrites.splice(0, tab.pendingWrites.length);
      for (const chunks of queued) {
        if (tab.generation !== myGeneration || phaseOf(tab) !== "live") {
          break;
        }
        enqueueChunks(tab, chunks);
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
      tab.pendingWrites = [];
      const message = errorText(err);
      if (keepBuffer) {
        tab.phase = "ended";
        showTabError(tab, message);
        refreshChrome();
        onChange();
      } else {
        removeTab(tab);
        sessionError.textContent = message;
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
    tab.pendingWrites = [];
    tab.queue.invalidate();
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
        showTabError(tab, errorText(err));
      });
    }, EXIT_REAP_DELAY_MS);
  }

  async function reconnectTab(tab: Tab): Promise<void> {
    if (tab.phase !== "ended") {
      return;
    }
    tab.queue.invalidate();
    tab.pendingWrites = [];
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
    tab.queue.invalidate();
    tab.pendingWrites = [];
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
    const keepLimitNotice =
      tab.error === "" &&
      tabs.length >= MAX_TABS &&
      sessionError.textContent === TAB_LIMIT_MESSAGE;
    if (!keepLimitNotice) {
      sessionError.textContent = tab.error;
    }
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
    const atLimit = tabs.length >= MAX_TABS;
    newTab.disabled = atLimit;
    newTab.title = atLimit ? TAB_LIMIT_MESSAGE : "";
    if (!atLimit && sessionError.textContent === TAB_LIMIT_MESSAGE) {
      sessionError.textContent = active?.error ?? "";
    }
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
      showTabError(tab, errorText(err));
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
        if (tabs.length >= MAX_TABS) {
          sessionError.textContent = TAB_LIMIT_MESSAGE;
          break;
        }
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
          void pasteClipboard(tab).catch((err: unknown) => {
            showTabError(tab, errorText(err));
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
    if (phaseOf(tab) === "ended") {
      return;
    }
    const generation = tab.generation;
    if (!navigator.clipboard?.readText) {
      throw new Error("无法读取剪贴板");
    }
    const text = await navigator.clipboard.readText();
    if (tab.generation !== generation || phaseOf(tab) === "ended") {
      return;
    }
    submitInput(tab, text);
  }

  function submitInput(tab: Tab, text: string): void {
    if (phaseOf(tab) === "ended" || text === "") {
      return;
    }
    const chunks = splitWriteChunks(text);
    if (chunks.length === 0) {
      return;
    }
    clearTabError(tab);
    if (tab.sessionId === "") {
      tab.pendingWrites.push(chunks);
      return;
    }
    enqueueChunks(tab, chunks);
  }

  function enqueueChunks(tab: Tab, chunks: readonly string[]): void {
    const generation = tab.generation;
    const id = tab.sessionId;
    void tab.queue.enqueue(
      chunks,
      async (chunk) => {
        if (generation !== tab.generation || tab.sessionId !== id || phaseOf(tab) === "ended") {
          return;
        }
        await writeSession(id, chunk);
      },
      (error) => {
        if (generation !== tab.generation || tab.sessionId !== id) {
          return;
        }
        showTabError(tab, errorText(error));
      },
    );
  }

  function showTabError(tab: Tab, message: string): void {
    tab.error = message;
    if (active === tab) {
      sessionError.textContent = message;
    }
  }

  function clearTabError(tab: Tab): void {
    const previous = tab.error;
    tab.error = "";
    if (active === tab && sessionError.textContent === previous) {
      sessionError.textContent = "";
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
    canOpenTab: () => tabs.length < MAX_TABS,
  };
}

function phaseOf(tab: Tab): Phase {
  return tab.phase;
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
