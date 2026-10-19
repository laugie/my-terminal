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

interface LiveConnect {
  id: string;
  host: HostEntry;
  ended: boolean;
}

export interface TerminalPanel {
  connect(host: HostEntry): Promise<void>;
  activeHostId(): string | null;
}

export function mountTerminal(
  panel: HTMLElement,
  onChange: () => void,
): TerminalPanel {
  const bar = el("div", "terminal-bar");
  const label = el("span", "terminal-label");
  label.textContent = "终端";
  const targetLabel = el("span", "terminal-target");
  const reconnect = document.createElement("button");
  reconnect.type = "button";
  reconnect.className = "button";
  reconnect.textContent = "重新连接";
  reconnect.hidden = true;
  bar.append(label, targetLabel, reconnect);

  const view = el("div", "terminal-view");
  view.setAttribute("role", "region");
  view.setAttribute("aria-label", "终端");
  const sessionError = el("p", "form-error");
  sessionError.setAttribute("role", "alert");
  panel.append(bar, view, sessionError);

  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let live: LiveConnect | null = null;
  let connecting = false;
  let generation = 0;
  let resizeFrame = 0;
  const pendingInput: string[] = [];

  reconnect.addEventListener("click", () => {
    if (live) {
      void connect(live.host);
    }
  });

  const observer = new ResizeObserver(() => {
    scheduleFit();
  });
  observer.observe(view);

  async function connect(host: HostEntry): Promise<void> {
    if (connecting) {
      return;
    }
    if (live && !live.ended && live.host.id === host.id) {
      term?.focus();
      return;
    }
    if (live && !live.ended && live.host.id !== host.id) {
      if (!window.confirm("结束当前会话并连接新主机？")) {
        return;
      }
    }

    const myGeneration = ++generation;
    connecting = true;
    reconnect.hidden = true;
    sessionError.textContent = "";
    panel.hidden = false;
    onChange();

    try {
      if (live?.id) {
        const previousId = live.id;
        live.ended = true;
        await closeSession(previousId);
      }
      if (myGeneration !== generation) {
        return;
      }

      const keepBuffer = live?.host.id === host.id && term !== null;
      const terminal = ensureTerminal();
      if (!keepBuffer) {
        terminal.reset();
      }
      pendingInput.length = 0;
      const record: LiveConnect = { id: "", host, ended: false };
      live = record;
      targetLabel.textContent = host.name;
      onChange();

      const opened = await openSession(sshTargetOf(host), (event) => {
        if (myGeneration !== generation) {
          return;
        }
        onSessionEvent(record, event);
      });
      if (myGeneration !== generation) {
        await closeSession(opened.id);
        return;
      }
      record.id = opened.id;
      if (record.ended) {
        await closeSession(opened.id);
        return;
      }
      const queued = pendingInput.splice(0, pendingInput.length);
      for (const data of queued) {
        if (record.ended) {
          break;
        }
        await writeSession(opened.id, data);
      }
      terminal.focus();
      scheduleFit();
    } catch (err) {
      if (myGeneration === generation) {
        sessionError.textContent = errorText(err);
        pendingInput.length = 0;
        if (live && live.id === "" && live.host.id === host.id) {
          live = null;
          onChange();
        }
      }
    } finally {
      if (myGeneration === generation) {
        connecting = false;
      }
    }
  }

  function onSessionEvent(record: LiveConnect, event: SessionEvent): void {
    const terminal = ensureTerminal();
    if (event.kind === "data") {
      terminal.write(new Uint8Array(event.bytes));
      return;
    }
    const codeText = event.code === null ? "未知" : String(event.code);
    terminal.write(`\r\n[会话已结束，退出码 ${codeText}]\r\n`);
    record.ended = true;
    pendingInput.length = 0;
    reconnect.hidden = false;
    onChange();
    if (record.id) {
      void closeSession(record.id).catch((err: unknown) => {
        if (live !== record) {
          return;
        }
        sessionError.textContent = errorText(err);
      });
    }
  }

  function ensureTerminal(): Terminal {
    if (term && fit) {
      return term;
    }
    const created = new Terminal({
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 14,
    });
    const addon = new FitAddon();
    created.loadAddon(addon);
    created.open(view);
    created.onData((data) => {
      if (!live || live.ended) {
        return;
      }
      if (live.id === "") {
        pendingInput.push(data);
        return;
      }
      const id = live.id;
      void writeSession(id, data).catch((err: unknown) => {
        if (!live || live.ended || live.id !== id) {
          return;
        }
        sessionError.textContent = errorText(err);
      });
    });
    term = created;
    fit = addon;
    return created;
  }

  function scheduleFit(): void {
    if (resizeFrame !== 0) {
      return;
    }
    resizeFrame = window.requestAnimationFrame(() => {
      resizeFrame = 0;
      applyFit();
    });
  }

  function applyFit(): void {
    if (!term || !fit || panel.hidden) {
      return;
    }
    fit.fit();
    if (!live || live.ended || live.id === "") {
      return;
    }
    const cols = term.cols;
    const rows = term.rows;
    if (cols < 1 || rows < 1) {
      return;
    }
    const id = live.id;
    void resizeSession(id, cols, rows).catch((err: unknown) => {
      if (!live || live.ended || live.id !== id) {
        return;
      }
      sessionError.textContent = errorText(err);
    });
  }

  return {
    connect,
    activeHostId: () => live?.host.id ?? null,
  };
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
