// 文件传输界面：左边本机、右边远端，下方是传输列表。每个会话标签一份。
// 面板之间的拖动用 pointer 事件（Tauri 接管了系统拖放，Windows 上 HTML5
// 拖放不可用）；从 Finder / 资源管理器拖入的文件由 main 转给 nativeDrag。

import { askCloseConfirm, closeConfirmOpen } from "./close-confirm.ts";
import { errorText } from "./hosts.ts";
import type { ShellTarget } from "./remote-shell.ts";
import {
  localDefaultDir,
  localList,
  sftpClose,
  sftpList,
  sftpOpen,
  transferCancel,
  transferStart,
  type FileEntry,
  type Listing,
  type NativeDrag,
  type TransferEvent,
} from "./sftp-api.ts";
import {
  TransferQueue,
  baseName,
  conflictingNames,
  dropDirection,
  formatBytes,
  formatModified,
  nextSelection,
  overwriteMessage,
  progressFraction,
  progressText,
  type Direction,
  type PaneSide,
  type TransferItem,
} from "./sftp-model.ts";
import { toolButton } from "./tab-bar.ts";
import { uiIcon } from "./ui-icons.ts";

/** Pointer travel before a press on a row becomes a drag. */
const DRAG_THRESHOLD_PX = 5;

/** Backend messages that mean the SFTP connection is gone. */
const LOST_MESSAGES = ["连接已断开", "文件传输连接已关闭"];

export interface SftpView {
  readonly root: HTMLElement;
  /** The view is on screen. Connects and loads both panes the first time. */
  show(): void;
  /** Files dragged in from Finder or Explorer while this view is showing. */
  nativeDrag(event: NativeDrag): void;
  /** Transfers queued or running. */
  busy(): number;
  /** Stop transfers and end the connection. */
  dispose(): void;
}

interface Pane {
  side: PaneSide;
  listing: Listing | null;
  selected: Set<string>;
  anchor: string | null;
  generation: number;
  rows: Map<string, HTMLElement>;
  element: HTMLElement;
  list: HTMLElement;
  pathInput: HTMLInputElement;
  upButton: HTMLButtonElement;
  refreshButton: HTMLButtonElement;
  sendButton: HTMLButtonElement;
  footer: HTMLElement;
  notice: HTMLElement;
}

interface DragState {
  side: PaneSide;
  pointerId: number;
  startX: number;
  startY: number;
  path: string;
  active: boolean;
  ghost: HTMLElement | null;
}

interface DropTarget {
  side: PaneSide;
  /** Folder the items go into. */
  dir: string;
  element: HTMLElement;
}

export function mountSftpView(
  parent: HTMLElement,
  target: ShellTarget,
  title: string,
  hooks: { onBack(): void },
): SftpView {
  const mac = isMac();
  const root = el("div", "sftp");
  root.hidden = true;

  const head = el("div", "sftp-head");
  const heading = el("p", "sftp-heading");
  heading.textContent = `文件传输 · ${title}`;
  const back = toolButton("terminal", "返回终端");
  back.addEventListener("click", () => {
    hooks.onBack();
  });
  head.append(heading, back);

  const panes = el("div", "sftp-panes");
  const local = makePane("local", "本机", "upload", "上传");
  const remote = makePane("remote", "远端", "download", "下载");
  panes.append(local.element, remote.element);

  const transfers = el("section", "sftp-transfers");
  const transfersHead = el("div", "sftp-transfers-head");
  const transfersTitle = el("span", "sftp-transfers-title");
  const clearButton = document.createElement("button");
  clearButton.type = "button";
  clearButton.className = "button ghost sftp-small";
  clearButton.textContent = "清除已完成";
  transfersHead.append(transfersTitle, clearButton);
  const transferList = el("ul", "sftp-transfer-list");
  transfers.append(transfersHead, transferList);

  root.append(head, panes, transfers);
  parent.append(root);

  const queue = new TransferQueue(2);
  const transferRows = new Map<string, HTMLElement>();
  /** Items whose Cancel was pressed before the backend id came back. */
  const cancelAsked = new Set<string>();

  let started = false;
  let disposed = false;
  let connection: { id: string; home: string } | null = null;
  let connecting: Promise<boolean> | null = null;
  let remoteMessage = "";
  let drag: DragState | null = null;
  let dropHighlight: HTMLElement | null = null;
  let suppressClick = false;

  clearButton.addEventListener("click", () => {
    queue.clearFinished();
    renderTransfers();
  });
  renderTransfers();

  // ------------------------------------------------------------ panes

  function makePane(
    side: PaneSide,
    label: string,
    sendIcon: "upload" | "download",
    sendLabel: string,
  ): Pane {
    const element = el("section", `sftp-pane sftp-pane-${side}`);
    element.dataset.side = side;
    const bar = el("div", "sftp-pane-head");
    const name = el("span", "sftp-pane-title");
    name.textContent = label;
    const tools = el("div", "sftp-pane-tools");
    const upButton = toolButton("up", "上一级", { iconOnly: true });
    const refreshButton = toolButton("reload", "刷新", { iconOnly: true });
    const sendButton = toolButton(sendIcon, sendLabel);
    sendButton.title =
      side === "local" ? "把选中的项目上传到远端当前文件夹" : "把选中的项目下载到本机当前文件夹";
    tools.append(upButton, refreshButton, sendButton);
    bar.append(name, tools);

    const pathForm = document.createElement("form");
    pathForm.className = "sftp-path";
    const pathInput = document.createElement("input");
    pathInput.type = "text";
    pathInput.spellcheck = false;
    pathInput.autocomplete = "off";
    pathInput.setAttribute("aria-label", `${label}路径`);
    pathForm.append(pathInput);

    const columns = el("div", "sftp-columns");
    columns.innerHTML = `<span>名称</span><span>大小</span><span>修改时间</span>`;
    const list = el("div", "sftp-list");
    list.tabIndex = 0;
    list.setAttribute("role", "listbox");
    list.setAttribute("aria-multiselectable", "true");
    list.setAttribute("aria-label", `${label}文件`);
    const notice = el("div", "sftp-notice");
    notice.hidden = true;
    const footer = el("p", "sftp-pane-foot");

    element.append(bar, pathForm, columns, list, notice, footer);
    const pane: Pane = {
      side,
      listing: null,
      selected: new Set(),
      anchor: null,
      generation: 0,
      rows: new Map(),
      element,
      list,
      pathInput,
      upButton,
      refreshButton,
      sendButton,
      footer,
      notice,
    };

    upButton.addEventListener("click", () => {
      goUp(pane);
    });
    refreshButton.addEventListener("click", () => {
      if (pane.side === "remote" && !connection) {
        void connect();
        return;
      }
      if (pane.listing) {
        void load(pane, pane.listing.path, true);
      }
    });
    sendButton.addEventListener("click", () => {
      sendSelected(pane);
    });
    pathForm.addEventListener("submit", (event) => {
      event.preventDefault();
      const typed = pathInput.value.trim();
      const value = typed === "此电脑" ? "" : typed;
      if (pane.side === "remote" && !connection) {
        return;
      }
      void load(pane, value === "" && pane.side === "remote" ? "/" : value, false);
      list.focus();
    });
    pathInput.addEventListener("blur", () => {
      pathInput.value = pane.listing?.path ?? "";
    });
    list.addEventListener("click", (event) => {
      if (suppressClick) {
        return;
      }
      const row = rowOf(event.target);
      if (!row) {
        setSelection(pane, new Set(), null);
        return;
      }
      const path = row.dataset.path ?? "";
      const next = nextSelection(order(pane), pane.selected, pane.anchor, path, {
        toggle: mac ? event.metaKey : event.ctrlKey,
        range: event.shiftKey,
      });
      setSelection(pane, next.selected, next.anchor);
    });
    list.addEventListener("dblclick", (event) => {
      const row = rowOf(event.target);
      const entry = row ? entryOf(pane, row.dataset.path ?? "") : undefined;
      if (!entry) {
        return;
      }
      if (entry.dir) {
        void load(pane, entry.path, false);
      } else {
        send(pane, [entry]);
      }
    });
    list.addEventListener("keydown", (event) => {
      onListKey(pane, event);
    });
    list.addEventListener("pointerdown", (event) => {
      onPointerDown(pane, event);
    });
    return pane;
  }

  function paneOf(side: PaneSide): Pane {
    return side === "local" ? local : remote;
  }

  function other(pane: Pane): Pane {
    return pane.side === "local" ? remote : local;
  }

  function order(pane: Pane): string[] {
    return pane.listing?.entries.map((entry) => entry.path) ?? [];
  }

  function entryOf(pane: Pane, path: string): FileEntry | undefined {
    return pane.listing?.entries.find((entry) => entry.path === path);
  }

  function selectedEntries(pane: Pane): FileEntry[] {
    return pane.listing?.entries.filter((entry) => pane.selected.has(entry.path)) ?? [];
  }

  async function load(pane: Pane, path: string, keepSelection: boolean): Promise<void> {
    const generation = ++pane.generation;
    pane.element.classList.add("sftp-loading");
    try {
      const listing =
        pane.side === "local"
          ? await localList(path)
          : await sftpList(connection?.id ?? "", path);
      if (generation !== pane.generation || disposed) {
        return;
      }
      const kept = keepSelection && pane.listing?.path === listing.path;
      pane.listing = listing;
      const paths = new Set(listing.entries.map((entry) => entry.path));
      pane.selected = kept
        ? new Set([...pane.selected].filter((item) => paths.has(item)))
        : new Set();
      if (!kept || (pane.anchor !== null && !paths.has(pane.anchor))) {
        pane.anchor = null;
      }
      showNotice(pane, "");
      renderPane(pane);
    } catch (err) {
      if (generation !== pane.generation || disposed) {
        return;
      }
      const message = errorText(err);
      if (pane.side === "remote" && isLost(message)) {
        lose(message);
        return;
      }
      pane.pathInput.value = pane.listing?.path ?? "";
      if (pane.listing) {
        flash(pane, message);
      } else {
        showNotice(pane, message);
        renderPane(pane);
      }
    } finally {
      if (generation === pane.generation) {
        pane.element.classList.remove("sftp-loading");
      }
    }
  }

  function goUp(pane: Pane): void {
    const parentPath = pane.listing?.parent;
    if (parentPath === null || parentPath === undefined) {
      return;
    }
    void load(pane, parentPath, false);
  }

  function renderPane(pane: Pane): void {
    const listing = pane.listing;
    pane.rows.clear();
    pane.list.replaceChildren();
    if (document.activeElement !== pane.pathInput) {
      pane.pathInput.value = listing ? (listing.path === "" ? "此电脑" : listing.path) : "";
    }
    if (listing) {
      for (const entry of listing.entries) {
        const row = renderRow(entry, pane.selected.has(entry.path));
        pane.rows.set(entry.path, row);
        pane.list.append(row);
      }
      if (listing.entries.length === 0) {
        const empty = el("p", "sftp-empty");
        empty.textContent = "空文件夹";
        pane.list.append(empty);
      }
    }
    syncPaneControls(pane);
  }

  function renderRow(entry: FileEntry, selected: boolean): HTMLElement {
    const row = el("div", "sftp-row");
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", selected ? "true" : "false");
    row.classList.toggle("sftp-row-selected", selected);
    row.dataset.path = entry.path;
    if (entry.dir) {
      row.dataset.dir = "true";
    }
    const name = el("span", "sftp-name");
    name.insertAdjacentHTML("beforeend", uiIcon(entry.dir ? "folder" : "file"));
    const text = el("span", "sftp-name-text");
    text.textContent = entry.name;
    name.append(text);
    if (entry.link) {
      const link = el("span", "sftp-link");
      link.textContent = "↪";
      link.title = "符号链接";
      name.append(link);
    }
    name.title = entry.name;
    const size = el("span", "sftp-size");
    size.textContent = entry.dir ? "" : formatBytes(entry.size);
    const time = el("span", "sftp-time");
    time.textContent = formatModified(entry.modified);
    row.append(name, size, time);
    return row;
  }

  function setSelection(pane: Pane, selected: Set<string>, anchor: string | null): void {
    pane.selected = selected;
    pane.anchor = anchor;
    for (const [path, row] of pane.rows) {
      const on = selected.has(path);
      row.classList.toggle("sftp-row-selected", on);
      row.setAttribute("aria-selected", on ? "true" : "false");
    }
    syncPaneControls(pane);
  }

  function syncPaneControls(pane: Pane): void {
    const listing = pane.listing;
    const live = pane.side === "local" || connection !== null;
    pane.upButton.disabled = !live || !listing || listing.parent === null;
    pane.refreshButton.disabled = pane.side === "local" ? !listing : connecting !== null;
    pane.refreshButton.title =
      pane.side === "remote" && !connection ? "重新连接" : "刷新";
    pane.pathInput.disabled = !live;
    const target = other(pane);
    pane.sendButton.disabled =
      pane.selected.size === 0 || !canReceive(target);
    const count = listing?.entries.length ?? 0;
    const parts: string[] = [];
    if (listing) {
      parts.push(`${count} 项`);
    }
    if (pane.selected.size > 0) {
      parts.push(`已选 ${pane.selected.size} 项`);
    }
    if (listing?.truncated) {
      parts.push(`只显示前 ${count} 项`);
    }
    pane.footer.textContent = parts.join(" · ");
    pane.footer.classList.remove("sftp-foot-error");
  }

  /** A pane that can take items: it shows a real folder (not the drive list). */
  function canReceive(pane: Pane): boolean {
    if (!pane.listing || pane.listing.path === "") {
      return false;
    }
    return pane.side === "local" || connection !== null;
  }

  function showNotice(pane: Pane, message: string, action?: { label: string; run(): void }): void {
    pane.notice.replaceChildren();
    pane.notice.hidden = message === "";
    pane.list.hidden = message !== "" && !pane.listing;
    if (message === "") {
      return;
    }
    const text = el("p", "sftp-notice-text");
    text.textContent = message;
    pane.notice.append(text);
    if (action) {
      const retry = document.createElement("button");
      retry.type = "button";
      retry.className = "button";
      retry.textContent = action.label;
      retry.addEventListener("click", action.run);
      pane.notice.append(retry);
    }
  }

  /** A short error in the footer; the listing stays. */
  function flash(pane: Pane, message: string): void {
    pane.footer.textContent = message;
    pane.footer.classList.add("sftp-foot-error");
  }

  function onListKey(pane: Pane, event: KeyboardEvent): void {
    if (closeConfirmOpen()) {
      return;
    }
    const paths = order(pane);
    const toggle = mac ? event.metaKey : event.ctrlKey;
    if (toggle && (event.key === "a" || event.key === "A")) {
      event.preventDefault();
      setSelection(pane, new Set(paths), paths[0] ?? null);
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (paths.length === 0) {
        return;
      }
      const current = pane.anchor ? paths.indexOf(pane.anchor) : -1;
      const step = event.key === "ArrowDown" ? 1 : -1;
      const index = Math.min(paths.length - 1, Math.max(0, current + step));
      const path = paths[index] ?? "";
      setSelection(pane, new Set([path]), path);
      pane.rows.get(path)?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const entries = selectedEntries(pane);
      const only = entries[0];
      if (entries.length === 1 && only?.dir) {
        void load(pane, only.path, false);
      } else if (entries.length > 0) {
        send(pane, entries);
      }
      return;
    }
    if (event.key === "Backspace") {
      event.preventDefault();
      goUp(pane);
      return;
    }
    if (event.key === "Escape") {
      setSelection(pane, new Set(), null);
    }
  }

  // ------------------------------------------------------------ connection

  async function connect(): Promise<boolean> {
    if (connection) {
      return true;
    }
    if (connecting) {
      return connecting;
    }
    remoteMessage = "";
    showNotice(remote, "正在连接…");
    remote.list.hidden = true;
    connecting = (async () => {
      try {
        const opened = await sftpOpen(target);
        if (disposed) {
          void sftpClose(opened.id).catch(() => {});
          return false;
        }
        connection = opened;
        showNotice(remote, "");
        remote.list.hidden = false;
        const start = remote.listing?.path || opened.home;
        await load(remote, start, true);
        return true;
      } catch (err) {
        if (disposed) {
          return false;
        }
        remoteMessage = errorText(err);
        showNotice(remote, remoteMessage, { label: "重新连接", run: () => void connect() });
        return false;
      } finally {
        connecting = null;
        syncPaneControls(remote);
        syncPaneControls(local);
      }
    })();
    syncPaneControls(remote);
    return connecting;
  }

  /** The connection dropped: show it and offer to reconnect. */
  function lose(message: string): void {
    if (connection) {
      void sftpClose(connection.id).catch(() => {});
    }
    connection = null;
    remoteMessage = message;
    showNotice(remote, `${message}。`, { label: "重新连接", run: () => void connect() });
    remote.list.hidden = false;
    syncPaneControls(remote);
    syncPaneControls(local);
  }

  // ------------------------------------------------------------ transfers

  function sendSelected(pane: Pane): void {
    send(pane, selectedEntries(pane));
  }

  /** Copy entries from `pane` into the other pane's current folder. */
  function send(pane: Pane, entries: FileEntry[]): void {
    const target = other(pane);
    if (entries.length === 0 || !target.listing || !canReceive(target)) {
      return;
    }
    void enqueue(
      dropDirection(pane.side, target.side) ?? "upload",
      entries.map((entry) => entry.path),
      target.listing.path,
      target.listing.entries,
    );
  }

  /**
   * Queue one transfer per item. When the destination listing is known,
   * ask before overwriting names that are already there.
   */
  async function enqueue(
    direction: Direction,
    sources: string[],
    destDir: string,
    existing: readonly FileEntry[] | null,
  ): Promise<void> {
    if (sources.length === 0 || destDir === "") {
      return;
    }
    if (existing) {
      const names = sources.map(baseName);
      const clash = conflictingNames(names, existing, direction === "download");
      if (clash.length > 0) {
        if (closeConfirmOpen()) {
          return;
        }
        const ok = await askCloseConfirm("覆盖同名项目", {
          message: overwriteMessage(clash),
          confirmLabel: "覆盖",
          focusCancel: true,
        });
        if (!ok || disposed) {
          return;
        }
      }
    }
    for (const source of sources) {
      queue.add({ direction, source, destDir });
    }
    renderTransfers();
    pump();
  }

  /** Start what the queue allows; reconnect first when needed. */
  function pump(): void {
    if (disposed) {
      return;
    }
    const waiting = queue.items.some((item) => item.status === "queued");
    if (!waiting) {
      return;
    }
    if (!connection) {
      if (connecting) {
        return;
      }
      void connect().then((ok) => {
        if (ok) {
          pump();
          return;
        }
        for (const item of queue.items) {
          if (item.status === "queued") {
            queue.fail(item.key, "connection", remoteMessage || "连接已断开", 0, 0);
          }
        }
        renderTransfers();
      });
      return;
    }
    for (const item of queue.startable()) {
      start(item, connection.id);
    }
    renderTransfers();
  }

  function start(item: TransferItem, connectionId: string): void {
    cancelAsked.delete(item.key);
    const onEvent = (event: TransferEvent) => {
      if (disposed) {
        return;
      }
      switch (event.kind) {
        case "progress":
          queue.progress(item.key, event.done, event.total, performance.now());
          renderTransfer(item);
          return;
        case "done":
          queue.finish(item.key, event.done, event.total);
          renderTransfers();
          refreshDestination(item);
          pump();
          return;
        case "failed":
          queue.fail(item.key, event.reason, event.message, event.done, event.total);
          renderTransfers();
          if (event.reason === "connection") {
            lose(event.message);
          }
          if (event.reason !== "connection" || connection) {
            pump();
          }
          return;
      }
    };
    transferStart(connectionId, item.direction, item.source, item.destDir, onEvent)
      .then((id) => {
        if (item.status !== "running") {
          return;
        }
        item.transferId = id;
        if (cancelAsked.has(item.key)) {
          void transferCancel(id).catch(() => {});
        }
      })
      .catch((err) => {
        const message = errorText(err);
        const lost = isLost(message);
        queue.fail(item.key, lost ? "connection" : "other", message, 0, 0);
        renderTransfers();
        if (lost) {
          lose(message);
        } else {
          pump();
        }
      });
  }

  function cancel(item: TransferItem): void {
    if (item.status === "queued") {
      queue.cancelQueued(item.key);
      renderTransfers();
      return;
    }
    if (item.status === "running") {
      cancelAsked.add(item.key);
      if (item.transferId) {
        void transferCancel(item.transferId).catch(() => {});
      }
      return;
    }
    // Failed or cancelled: Cancel dismisses the row.
    queue.remove(item.key);
    renderTransfers();
  }

  function retry(item: TransferItem): void {
    if (queue.retry(item.key)) {
      renderTransfers();
      pump();
    }
  }

  /** Show new files where they landed, keeping the selection. */
  function refreshDestination(item: TransferItem): void {
    const pane = paneOf(item.direction === "upload" ? "remote" : "local");
    if (pane.listing && pane.listing.path === item.destDir) {
      void load(pane, pane.listing.path, true);
    }
  }

  function renderTransfers(): void {
    const items = queue.items;
    const keys = new Set(items.map((item) => item.key));
    for (const [key, row] of transferRows) {
      if (!keys.has(key)) {
        row.remove();
        transferRows.delete(key);
      }
    }
    for (const item of items) {
      renderTransfer(item);
    }
    // Keep DOM order equal to queue order.
    items.forEach((item, index) => {
      const row = transferRows.get(item.key);
      if (row && transferList.children[index] !== row) {
        transferList.insertBefore(row, transferList.children[index] ?? null);
      }
    });
    const active = queue.active();
    transfersTitle.textContent = active > 0 ? `传输（${active} 项进行中）` : "传输";
    clearButton.disabled = !items.some(
      (item) => item.status === "done" || item.status === "cancelled",
    );
    transfers.classList.toggle("sftp-transfers-empty", items.length === 0);
  }

  function renderTransfer(item: TransferItem): void {
    let row = transferRows.get(item.key);
    if (!row) {
      row = buildTransferRow(item);
      transferRows.set(item.key, row);
      transferList.append(row);
    }
    row.dataset.status = item.status;
    const text = row.querySelector<HTMLElement>(".sftp-transfer-text");
    if (text) {
      text.textContent = progressText(item);
      text.title = item.status === "failed" ? progressText(item) : "";
    }
    const fill = row.querySelector<HTMLElement>(".sftp-bar-fill");
    if (fill) {
      // A failed bar fills up in red so the failure is visible at any progress.
      const fraction = item.status === "failed" ? 1 : progressFraction(item);
      fill.style.width = `${(fraction * 100).toFixed(1)}%`;
    }
    const bar = row.querySelector<HTMLElement>(".sftp-bar");
    bar?.setAttribute("aria-valuenow", String(Math.round(progressFraction(item) * 100)));
    const show = (action: string, on: boolean) => {
      const node = row?.querySelector<HTMLButtonElement>(`[data-action='${action}']`);
      if (node) {
        node.hidden = !on;
      }
    };
    // Failed rows offer Retry and Cancel (Cancel removes the row).
    show("retry", item.status === "failed" || item.status === "cancelled");
    show("cancel", item.status === "queued" || item.status === "running" || item.status === "failed");
    show("dismiss", item.status === "done" || item.status === "cancelled");
  }

  function buildTransferRow(item: TransferItem): HTMLElement {
    const row = el("li", "sftp-transfer");
    const line = el("div", "sftp-transfer-line");
    const icon = el("span", "sftp-transfer-icon");
    icon.innerHTML = uiIcon(item.direction === "upload" ? "upload" : "download");
    icon.title = item.direction === "upload" ? "上传" : "下载";
    const name = el("span", "sftp-transfer-name");
    name.textContent = item.name;
    name.title = `${item.source} → ${item.destDir}`;
    const text = el("span", "sftp-transfer-text");
    const actions = el("span", "sftp-transfer-actions");
    const retryButton = smallButton("重试", "retry");
    retryButton.addEventListener("click", () => {
      retry(item);
    });
    const cancelButton = smallButton("取消", "cancel");
    cancelButton.addEventListener("click", () => {
      cancel(item);
    });
    cancelButton.title = "停止或放弃这次传输";
    const dismiss = smallButton("移除", "dismiss");
    dismiss.addEventListener("click", () => {
      queue.remove(item.key);
      renderTransfers();
    });
    actions.append(retryButton, cancelButton, dismiss);
    line.append(icon, name, text, actions);
    const bar = el("div", "sftp-bar");
    bar.setAttribute("role", "progressbar");
    bar.setAttribute("aria-valuemin", "0");
    bar.setAttribute("aria-valuemax", "100");
    bar.setAttribute("aria-label", item.name);
    const fill = el("div", "sftp-bar-fill");
    bar.append(fill);
    row.append(line, bar);
    return row;
  }

  // ------------------------------------------------------------ drag between panes

  function onPointerDown(pane: Pane, event: PointerEvent): void {
    if (event.button !== 0 || drag) {
      return;
    }
    const row = rowOf(event.target);
    if (!row) {
      return;
    }
    drag = {
      side: pane.side,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      path: row.dataset.path ?? "",
      active: false,
      ghost: null,
    };
    document.addEventListener("pointermove", onPointerMove, true);
    document.addEventListener("pointerup", onPointerUp, true);
    document.addEventListener("pointercancel", endDrag, true);
    document.addEventListener("keydown", onDragKey, true);
  }

  function onPointerMove(event: PointerEvent): void {
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    if (!drag.active) {
      const moved = Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY);
      if (moved < DRAG_THRESHOLD_PX) {
        return;
      }
      const pane = paneOf(drag.side);
      if (!pane.selected.has(drag.path)) {
        setSelection(pane, new Set([drag.path]), drag.path);
      }
      drag.active = true;
      const ghost = el("div", "sftp-ghost");
      const count = pane.selected.size;
      ghost.textContent =
        count === 1 ? baseName(drag.path) : `${count} 项`;
      document.body.append(ghost);
      drag.ghost = ghost;
      root.classList.add("sftp-dragging");
    }
    event.preventDefault();
    if (drag.ghost) {
      drag.ghost.style.transform = `translate(${event.clientX + 14}px, ${event.clientY + 10}px)`;
    }
    const found = dropTargetAt(event.clientX, event.clientY);
    highlight(found && found.side !== drag.side ? found.element : null);
  }

  function onPointerUp(event: PointerEvent): void {
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    const state = drag;
    endDrag();
    if (!state.active) {
      return;
    }
    // The click that follows this pointerup must not change the selection.
    suppressClick = true;
    window.setTimeout(() => {
      suppressClick = false;
    }, 0);
    const found = dropTargetAt(event.clientX, event.clientY);
    if (!found || found.side === state.side) {
      return;
    }
    const from = paneOf(state.side);
    const into = paneOf(found.side);
    const direction = dropDirection(from.side, into.side);
    if (!direction || !canReceive(into)) {
      return;
    }
    const sources = selectedEntries(from).map((entry) => entry.path);
    const known = into.listing && into.listing.path === found.dir ? into.listing.entries : null;
    void enqueue(direction, sources, found.dir, known);
  }

  function onDragKey(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      endDrag();
    }
  }

  function endDrag(): void {
    document.removeEventListener("pointermove", onPointerMove, true);
    document.removeEventListener("pointerup", onPointerUp, true);
    document.removeEventListener("pointercancel", endDrag, true);
    document.removeEventListener("keydown", onDragKey, true);
    drag?.ghost?.remove();
    drag = null;
    root.classList.remove("sftp-dragging");
    highlight(null);
  }

  /** The pane or folder row under a point, with the folder items would go into. */
  function dropTargetAt(x: number, y: number): DropTarget | null {
    const hit = document.elementFromPoint(x, y);
    if (!(hit instanceof Element) || !root.contains(hit)) {
      return null;
    }
    const paneElement = hit.closest<HTMLElement>(".sftp-pane");
    const side = paneElement?.dataset.side;
    if (!paneElement || (side !== "local" && side !== "remote")) {
      return null;
    }
    const pane = paneOf(side);
    if (!canReceive(pane) || !pane.listing) {
      return null;
    }
    const row = rowOf(hit);
    if (row && row.dataset.dir === "true" && row.dataset.path) {
      // Dropping a folder onto itself is not a target.
      if (!(drag && drag.side === side)) {
        return { side, dir: row.dataset.path, element: row };
      }
    }
    return { side, dir: pane.listing.path, element: pane.list };
  }

  function highlight(element: HTMLElement | null): void {
    if (dropHighlight === element) {
      return;
    }
    dropHighlight?.classList.remove("sftp-drop-target");
    dropHighlight = element;
    element?.classList.add("sftp-drop-target");
  }

  // ------------------------------------------------------------ drag from Finder / Explorer

  function nativeDrag(event: NativeDrag): void {
    if (root.hidden) {
      return;
    }
    if (event.type === "leave") {
      highlight(null);
      return;
    }
    const found = dropTargetAt(event.x, event.y);
    const usable = found && found.side === "remote" ? found : null;
    if (event.type === "enter" || event.type === "over") {
      highlight(usable ? usable.element : null);
      return;
    }
    highlight(null);
    if (!usable) {
      if (found?.side === "local") {
        flash(local, "从 Finder / 资源管理器拖到右侧远端面板即可上传");
      } else if (!connection) {
        flash(remote, "远端未连接，无法上传");
      }
      return;
    }
    const known =
      remote.listing && remote.listing.path === usable.dir ? remote.listing.entries : null;
    void enqueue("upload", event.paths, usable.dir, known);
  }

  // ------------------------------------------------------------ lifecycle

  async function startLocal(): Promise<void> {
    let dir = "";
    try {
      dir = await localDefaultDir();
    } catch (err) {
      showNotice(local, errorText(err));
      return;
    }
    await load(local, dir, false);
  }

  return {
    root,
    show(): void {
      root.hidden = false;
      if (!started) {
        started = true;
        renderPane(local);
        renderPane(remote);
        void startLocal();
        void connect();
      }
    },
    nativeDrag,
    busy: () => queue.active(),
    dispose(): void {
      disposed = true;
      endDrag();
      for (const item of queue.items) {
        if (item.status === "running" && item.transferId) {
          void transferCancel(item.transferId).catch(() => {});
        }
      }
      if (connection) {
        void sftpClose(connection.id).catch(() => {});
        connection = null;
      }
      root.remove();
    },
  };
}

function isLost(message: string): boolean {
  return LOST_MESSAGES.some((text) => message.startsWith(text));
}

function rowOf(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(".sftp-row") : null;
}

function isMac(): boolean {
  const withData = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = withData.userAgentData?.platform ?? navigator.platform;
  return /mac/i.test(platform);
}

function smallButton(label: string, action: string): HTMLButtonElement {
  const node = document.createElement("button");
  node.type = "button";
  node.className = "button ghost sftp-small";
  node.dataset.action = action;
  node.textContent = label;
  return node;
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  return node;
}
