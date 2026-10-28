import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { fontOf, themeOf, type FontChoice, type ThemeChoice } from "./appearance.ts";
import {
  clipboardKey,
  CopyRelay,
  pasteConfirmText,
  pasteNeedsConfirm,
  rightClickAction,
  type ClipboardAction,
} from "./clipboard.ts";
import { readClipboardText, writeClipboardText } from "./clipboard-api.ts";
import { askCloseConfirm, closeConfirmOpen } from "./close-confirm.ts";
import { errorText, type HostEntry } from "./hosts.ts";
import {
  closeSession,
  openSession,
  resizeSession,
  sshTargetOf,
  writeSession,
  type SessionEvent,
} from "./session.ts";
import { shortcutAction, type ShortcutAction } from "./tabs.ts";
import { openTopologyDevice } from "./topology.ts";
import { splitWriteChunks } from "./write-chunks.ts";
import { WriteQueue } from "./write-queue.ts";

/** Extra time after Exit before the process is reaped, so a late chunk can land. */
const EXIT_REAP_DELAY_MS = 300;


type Phase = "connecting" | "live" | "ended" | "failed";

type TabOrigin =
  | { kind: "host"; host: HostEntry }
  | { kind: "device"; name: string; connectionName?: string };

interface Session {
  key: string;
  origin: TabOrigin;
  phase: Phase;
  sessionId: string;
  generation: number;
  pendingWrites: string[][];
  queue: WriteQueue;
  error: string;
  term: Terminal;
  fit: FitAddon;
  view: HTMLElement;
  reapTimer: number;
  discarded: boolean;
}

export interface TerminalHost {
  /** Fit the active tab again after a zoom change. */
  refit(): void;
  /** Recolor and re-font every tab, open or not, then fit again. */
  setAppearance(theme: ThemeChoice, font: FontChoice): void;
  openDevice(key: string, name: string, connectionName?: string): Promise<{ id: string }>;
  openHost(key: string, host: HostEntry): Promise<{ id: string }>;
  discard(key: string): void;
  activate(key: string | null): void;
  reconnect(key: string): Promise<void>;
}

export function mountTerminal(
  panel: HTMLElement,
  hooks: {
    onChange(): void;
    onConnected(key: string, id: string): void;
    onExit(key: string, code: number | null): void;
    onFailed(key: string, message: string): void;
    onShortcut(action: ShortcutAction): void;
    onNew(): void;
    onCycle(step: number): void;
  },
): TerminalHost {
  const stack = el("div", "terminal-stack");
  const sessionError = el("p", "form-error");
  sessionError.setAttribute("role", "alert");
  panel.append(stack, sessionError);

  const sessions = new Map<string, Session>();
  let activeKey: string | null = null;
  let resizeFrame = 0;
  let theme = themeOf(undefined);
  let font = fontOf(undefined);
  paintBackground();

  const observer = new ResizeObserver(() => {
    scheduleFit();
  });
  observer.observe(stack);
  window.addEventListener("keydown", onWindowKeyDown, true);
  // Edit menu and context-menu pastes arrive here. Taken before xterm, which
  // would paste without asking.
  document.addEventListener("paste", onPaste, true);
  // Every copy in a terminal tab carries xterm's selection text, whichever
  // way the copy started. WebKit enables Edit > Copy only when the page
  // claims beforecopy.
  const copyRelay = new CopyRelay();
  document.addEventListener("copy", onCopy, true);
  document.addEventListener("beforecopy", onBeforeCopy, true);

  async function openDevice(
    key: string,
    name: string,
    connectionName?: string,
  ): Promise<{ id: string }> {
    const selected = connectionName ? connectionName : undefined;
    return openOn(ensure(key, { kind: "device", name, connectionName: selected }), false);
  }

  async function openHost(key: string, host: HostEntry): Promise<{ id: string }> {
    return openOn(ensure(key, { kind: "host", host }), false);
  }

  function ensure(key: string, origin: TabOrigin): Session {
    const existing = sessions.get(key);
    if (existing) {
      return existing;
    }
    return createSession(key, origin);
  }

  function createSession(key: string, origin: TabOrigin): Session {
    const view = el("div", "terminal-view");
    view.hidden = true;
    stack.append(view);
    const term = new Terminal({
      fontFamily: font.stack,
      fontSize: 14,
      lineHeight: 1.15,
      cursorBlink: true,
      scrollback: 5000,
      theme: { ...theme.palette },
      minimumContrastRatio: theme.minimumContrastRatio ?? 1,
      // Windows uses right-click for copy and paste, so it must not select a word first.
      rightClickSelectsWord: shortcutPlatform() === "mac",
      // Option-drag selects even when a program such as tmux or vim takes the mouse.
      // Shift-drag does the same on Windows.
      macOptionClickForcesSelection: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const session: Session = {
      key,
      origin,
      phase: "connecting",
      sessionId: "",
      generation: 0,
      pendingWrites: [],
      queue: new WriteQueue(),
      error: "",
      term,
      fit,
      view,
      reapTimer: 0,
      discarded: false,
    };
    sessions.set(key, session);
    term.attachCustomKeyEventHandler((event) => {
      if (event.isComposing) {
        return true;
      }
      const action = shortcutAction(event, shortcutPlatform());
      if (action === null) {
        return true;
      }
      // xterm also calls this for keyup and keypress. Act once, on keydown.
      if (event.type === "keydown" && !event.defaultPrevented) {
        hooks.onShortcut(action);
      }
      return false;
    });
    // xterm stops the paste event and feeds the whole clipboard string through
    // onData. Chunk on this path so a menu paste stays within the write limit.
    term.onData((data) => {
      submitInput(session, data);
    });
    if (shortcutPlatform() === "windows") {
      // Taken before xterm and the webview's own menu.
      view.addEventListener(
        "contextmenu",
        (event) => {
          event.preventDefault();
          event.stopPropagation();
          runClipboard(session, rightClickAction(term.hasSelection()));
        },
        true,
      );
    }
    term.open(view);
    return session;
  }

  async function openOn(session: Session, keepBuffer: boolean): Promise<{ id: string }> {
    const myGeneration = ++session.generation;
    session.queue.invalidate();
    window.clearTimeout(session.reapTimer);
    session.reapTimer = 0;
    session.phase = "connecting";
    session.sessionId = "";
    session.pendingWrites = [];
    session.discarded = false;
    clearError(session);
    if (!keepBuffer) {
      session.term.reset();
    }
    hooks.onChange();
    try {
      const opened = await openOrigin(session.origin, (event) => {
        if (session.generation !== myGeneration || session.discarded) {
          return;
        }
        onSessionEvent(session, event, myGeneration);
      });
      if (session.discarded) {
        return opened;
      }
      if (session.generation !== myGeneration) {
        await closeSession(opened.id);
        throw new Error("会话已取消");
      }
      session.sessionId = opened.id;
      if (currentPhase(session) === "ended") {
        session.pendingWrites = [];
        session.queue.invalidate();
        scheduleReap(session, opened.id, myGeneration);
        return opened;
      }
      session.phase = "live";
      hooks.onConnected(session.key, opened.id);
      const queued = session.pendingWrites.splice(0, session.pendingWrites.length);
      for (const chunks of queued) {
        if (session.generation !== myGeneration || session.phase !== "live") {
          break;
        }
        enqueueChunks(session, chunks);
      }
      hooks.onChange();
      if (activeKey === session.key) {
        scheduleFit();
        session.term.focus();
      }
      return opened;
    } catch (err) {
      if (session.discarded || session.generation !== myGeneration) {
        throw err;
      }
      session.pendingWrites = [];
      const message = errorText(err);
      session.phase = "failed";
      showError(session, message);
      hooks.onFailed(session.key, message);
      hooks.onChange();
      throw new Error(message);
    }
  }

  function onSessionEvent(session: Session, event: SessionEvent, generation: number): void {
    if (event.kind === "data") {
      session.term.write(new Uint8Array(event.bytes));
      return;
    }
    if (session.phase === "ended" || session.phase === "failed") {
      return;
    }
    const codeText = event.code === null ? "未知" : String(event.code);
    session.term.write(`\r\n[会话已结束，退出码 ${codeText}]\r\n`);
    session.phase = "ended";
    session.pendingWrites = [];
    session.queue.invalidate();
    hooks.onExit(session.key, event.code);
    hooks.onChange();
    if (session.sessionId !== "") {
      scheduleReap(session, session.sessionId, generation);
    }
  }

  function scheduleReap(session: Session, id: string, generation: number): void {
    window.clearTimeout(session.reapTimer);
    session.reapTimer = window.setTimeout(() => {
      session.reapTimer = 0;
      if (session.generation !== generation || session.sessionId !== id || session.discarded) {
        return;
      }
      void closeSession(id).catch((err: unknown) => {
        if (session.generation !== generation) {
          return;
        }
        showError(session, errorText(err));
      });
    }, EXIT_REAP_DELAY_MS);
  }

  async function reconnect(key: string): Promise<void> {
    const session = sessions.get(key);
    if (!session || session.phase !== "ended") {
      return;
    }
    session.queue.invalidate();
    session.pendingWrites = [];
    window.clearTimeout(session.reapTimer);
    session.reapTimer = 0;
    const oldId = session.sessionId;
    if (oldId !== "") {
      try {
        await closeSession(oldId);
      } catch (err) {
        showError(session, errorText(err));
      }
    }
    if (activeKey === key) {
      session.term.focus();
    }
    await openOn(session, true);
  }

  function discard(key: string): void {
    const session = sessions.get(key);
    if (!session) {
      return;
    }
    session.discarded = true;
    session.generation += 1;
    session.queue.invalidate();
    session.pendingWrites = [];
    window.clearTimeout(session.reapTimer);
    session.term.dispose();
    session.view.remove();
    sessions.delete(key);
    if (activeKey === key) {
      activeKey = null;
      stack.classList.remove("has-tabs");
    }
    if (sessionError.textContent === session.error) {
      sessionError.textContent = "";
    }
  }

  function activate(key: string | null): void {
    activeKey = key;
    let shown = false;
    for (const session of sessions.values()) {
      const on = session.key === key;
      session.view.hidden = !on;
      if (on) {
        shown = true;
      }
    }
    stack.classList.toggle("has-tabs", shown);
    const session = key ? sessions.get(key) : undefined;
    sessionError.textContent = session?.error ?? "";
    hooks.onChange();
    scheduleFit();
    if (session && session.phase !== "ended" && session.phase !== "failed") {
      session.term.focus();
    }
  }

  function scheduleFit(): void {
    if (resizeFrame !== 0) {
      return;
    }
    resizeFrame = window.requestAnimationFrame(() => {
      resizeFrame = 0;
      const session = activeKey ? sessions.get(activeKey) : undefined;
      if (session) {
        applyFit(session);
      }
    });
  }

  function applyFit(session: Session): void {
    if (session.key !== activeKey || session.view.hidden) {
      return;
    }
    session.fit.fit();
    if (session.phase !== "live" || session.sessionId === "") {
      return;
    }
    const cols = session.term.cols;
    const rows = session.term.rows;
    if (cols < 1 || rows < 1) {
      return;
    }
    const id = session.sessionId;
    void resizeSession(id, cols, rows).catch((err: unknown) => {
      if (session.phase !== "live" || session.sessionId !== id) {
        return;
      }
      showError(session, errorText(err));
    });
  }

  function onWindowKeyDown(event: KeyboardEvent): void {
    if (event.isComposing || event.altKey) {
      return;
    }
    const zoom = shortcutAction(event, shortcutPlatform());
    if (zoom?.type === "zoom") {
      // Zoom keys type nothing in a form field, so they work on the login screen too.
      consume(event);
      hooks.onShortcut(zoom);
      return;
    }
    if (typingInForm(event)) {
      return;
    }
    if (event.ctrlKey && !event.metaKey && event.key === "Tab") {
      consume(event);
      hooks.onCycle(event.shiftKey ? -1 : 1);
      return;
    }
    const action = shortcutAction(event, shortcutPlatform());
    if (action) {
      consume(event);
      if (!(action.type === "close" && event.repeat)) {
        hooks.onShortcut(action);
      }
      return;
    }
    const session = active();
    const clip = clipboardKey(event, shortcutPlatform(), session?.term.hasSelection() ?? false);
    if (clip) {
      // Without a session tab the key stays with the page.
      if (session) {
        consume(event);
        if (!closeConfirmOpen()) {
          runClipboard(session, clip);
        }
      }
      return;
    }
    const mac = shortcutPlatform() === "mac";
    if (mac) {
      if (!event.metaKey || event.ctrlKey) {
        return;
      }
    } else if (!event.ctrlKey || event.metaKey || !event.shiftKey) {
      return;
    }
    if (event.key.toUpperCase() === "T") {
      consume(event);
      hooks.onNew();
    }
  }

  function runClipboard(session: Session, action: ClipboardAction): void {
    if (action === "paste") {
      void pasteFromClipboard(session).catch((err: unknown) => {
        showError(session, errorText(err));
      });
      return;
    }
    const text = session.term.getSelection();
    if (action === "copy-clear") {
      session.term.clearSelection();
    }
    if (text === "") {
      return;
    }
    // The webview's own copy command writes the clipboard before this key
    // press ends. The app's clipboard command is only the fallback.
    if (copyRelay.copy(text, () => document.execCommand("copy"))) {
      return;
    }
    void writeClipboardText(text).catch((err: unknown) => {
      showError(session, errorText(err));
    });
  }

  function onCopy(event: ClipboardEvent): void {
    const session = active();
    if (!session || !event.clipboardData || typingInForm(event)) {
      return;
    }
    const text = copyRelay.take(() => session.term.getSelection());
    if (text === null) {
      return;
    }
    event.clipboardData.setData("text/plain", text);
    consume(event);
  }

  function onBeforeCopy(event: Event): void {
    const session = active();
    if (session?.term.hasSelection() && !typingInForm(event)) {
      event.preventDefault();
    }
  }

  async function pasteFromClipboard(session: Session): Promise<void> {
    if (blocksInput(session)) {
      return;
    }
    const generation = session.generation;
    const text = await readClipboardText();
    if (session.generation !== generation) {
      return;
    }
    await pasteText(session, text);
  }

  function onPaste(event: ClipboardEvent): void {
    const session = active();
    if (!session || closeConfirmOpen() || typingInForm(event)) {
      return;
    }
    consume(event);
    void pasteText(session, event.clipboardData?.getData("text/plain") ?? "");
  }

  /** Multi-line text asks first. `term.paste` adds bracketed paste when the shell asks for it. */
  async function pasteText(session: Session, text: string): Promise<void> {
    if (text === "" || blocksInput(session) || closeConfirmOpen()) {
      return;
    }
    if (pasteNeedsConfirm(text)) {
      const generation = session.generation;
      const confirmed = await askCloseConfirm(originTitle(session.origin), pasteConfirmText(text));
      if (activeKey === session.key) {
        session.term.focus();
      }
      if (!confirmed || session.generation !== generation || blocksInput(session)) {
        return;
      }
    }
    session.term.paste(text);
  }

  function submitInput(session: Session, text: string): void {
    if (blocksInput(session) || text === "") {
      return;
    }
    const chunks = splitWriteChunks(text);
    if (chunks.length === 0) {
      return;
    }
    clearError(session);
    if (session.sessionId === "") {
      session.pendingWrites.push(chunks);
      return;
    }
    enqueueChunks(session, chunks);
  }

  function enqueueChunks(session: Session, chunks: readonly string[]): void {
    const generation = session.generation;
    const id = session.sessionId;
    void session.queue.enqueue(
      chunks,
      async (chunk) => {
        if (generation !== session.generation || session.sessionId !== id || blocksInput(session)) {
          return;
        }
        await writeSession(id, chunk);
      },
      (error) => {
        if (generation !== session.generation || session.sessionId !== id) {
          return;
        }
        showError(session, errorText(error));
      },
    );
  }

  function showError(session: Session, message: string): void {
    session.error = message;
    if (activeKey === session.key) {
      sessionError.textContent = message;
    }
  }

  function clearError(session: Session): void {
    const previous = session.error;
    session.error = "";
    if (activeKey === session.key && sessionError.textContent === previous) {
      sessionError.textContent = "";
    }
  }

  function active(): Session | undefined {
    if (!activeKey) {
      return undefined;
    }
    return sessions.get(activeKey);
  }

  function setAppearance(nextTheme: ThemeChoice, nextFont: FontChoice): void {
    theme = nextTheme;
    font = nextFont;
    paintBackground();
    for (const session of sessions.values()) {
      session.term.options.theme = { ...theme.palette };
      session.term.options.minimumContrastRatio = theme.minimumContrastRatio ?? 1;
      // A new family changes the cell size, so rows and columns follow.
      session.term.options.fontFamily = font.stack;
    }
    refit();
  }

  /** The panel shows the theme background around the cell grid. */
  function paintBackground(): void {
    panel.style.setProperty("--term-bg", theme.palette.background);
  }

  function refit(): void {
    scheduleFit();
    // xterm measures the cell size again only after the pixel ratio change
    // reaches it, so fit once more a moment later.
    window.setTimeout(scheduleFit, 150);
  }

  return {
    refit,
    setAppearance,
    openDevice,
    openHost,
    discard,
    activate,
    reconnect,
  };
}

function openOrigin(
  origin: TabOrigin,
  onEvent: (event: SessionEvent) => void,
): Promise<{ id: string }> {
  if (origin.kind === "host") {
    return openSession(sshTargetOf(origin.host), onEvent);
  }
  return openTopologyDevice(origin.name, onEvent, origin.connectionName);
}

function originTitle(origin: TabOrigin): string {
  if (origin.kind === "host") {
    return origin.host.name;
  }
  return origin.connectionName ? `${origin.name} · ${origin.connectionName}` : origin.name;
}

function currentPhase(session: Session): Phase {
  return session.phase;
}

function blocksInput(session: Session): boolean {
  return session.phase === "ended" || session.phase === "failed";
}

export function shortcutPlatform(): "mac" | "windows" {
  const withData = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = withData.userAgentData?.platform ?? navigator.platform;
  return /mac/i.test(platform) ? "mac" : "windows";
}

function typingInForm(event: Event): boolean {
  const target = event.target;
  if (!(target instanceof HTMLElement) || target.closest(".terminal-panel, .xterm")) {
    return false;
  }
  return target.matches("input, textarea, select, [contenteditable='true']");
}

function consume(event: Event): void {
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
