/** Choosing a direct / ProxyJump session or a command relay. No DOM. */

export const DEFAULT_CONNECTION_LABEL = "默认（直连/ProxyJump）";

export interface ConnectionChoice {
  /** `null` is the device's existing direct or ProxyJump method. */
  connectionName: string | null;
  label: string;
}

export type MenuAction =
  | { type: "move"; index: number }
  | { type: "choose" }
  | { type: "close" }
  | { type: "ignore" };

/** No menu when the device has no extra relays. */
export function connectionChoices(
  names: readonly string[] | undefined,
): ConnectionChoice[] | null {
  if (!names || names.length === 0) {
    return null;
  }
  return [
    { connectionName: null, label: DEFAULT_CONNECTION_LABEL },
    ...names.map((name) => ({ connectionName: name, label: name })),
  ];
}

export function deviceTabTitle(deviceName: string, connectionName?: string | null): string {
  if (!connectionName) {
    return deviceName;
  }
  return `${deviceName} · ${connectionName}`;
}

/** Fields for `open_session`. An empty connection name is the default method. */
export function topologySessionFields(
  deviceName: string,
  connectionName?: string | null,
): { deviceName: string; connectionName?: string } {
  if (!connectionName) {
    return { deviceName };
  }
  return { deviceName, connectionName };
}

/**
 * Same allowlist as the backend relay template.
 * Returns a Chinese error, or `null` when the template is accepted.
 * The backend still checks the file; this does not replace that.
 */
export function relayCommandError(command: string): string | null {
  if (command.length === 0) {
    return "中转命令不能为空";
  }
  if (command.startsWith("-")) {
    return "中转命令不能以 - 开头";
  }
  let hasHost = false;
  let hasPort = false;
  for (let index = 0; index < command.length; ) {
    const ch = command[index] ?? "";
    if (ch === "%") {
      const next = command[index + 1];
      if (next === undefined) {
        return "中转命令包含未知占位符";
      }
      if (next === "h") {
        hasHost = true;
      } else if (next === "p") {
        hasPort = true;
      } else if (next !== "%") {
        return `中转命令包含未知占位符 %${next}`;
      }
      index += 2;
      continue;
    }
    const code = ch.charCodeAt(0);
    if (code < 0x20 || code === 0x7f) {
      return "中转命令不能包含换行或控制字符";
    }
    if (!/[A-Za-z0-9._/:=@+\- ]/.test(ch)) {
      return `中转命令包含不允许的字符 ${ch}`;
    }
    index += 1;
  }
  if (!hasHost || !hasPort) {
    return "中转命令必须同时包含 %h 和 %p";
  }
  return null;
}

export function menuAction(key: string, choiceCount: number, index: number): MenuAction {
  if (choiceCount <= 0) {
    return { type: "ignore" };
  }
  if (key === "ArrowDown") {
    return { type: "move", index: (index + 1) % choiceCount };
  }
  if (key === "ArrowUp") {
    return { type: "move", index: (index - 1 + choiceCount) % choiceCount };
  }
  if (key === "Enter" || key === " ") {
    return { type: "choose" };
  }
  if (key === "Escape") {
    return { type: "close" };
  }
  return { type: "ignore" };
}

/** Markup for the jump / relay chooser. The command field is a local check only. */
export function renderConnectionMenu(deviceName: string, names: readonly string[]): string {
  const choices = connectionChoices(names) ?? [
    { connectionName: null, label: DEFAULT_CONNECTION_LABEL },
  ];
  const items = choices
    .map((choice) => {
      const value = choice.connectionName === null ? "" : escapeHtml(choice.connectionName);
      return `<button type="button" class="button" role="menuitem" data-connection="${value}">${escapeHtml(choice.label)}</button>`;
    })
    .join("");
  return (
    `<div class="connection-menu" role="menu" aria-label="连接方式">` +
    `<p class="connection-menu-title">${escapeHtml(deviceName)}</p>` +
    items +
    `<label class="relay-command-label">中转命令<input class="relay-command-input" type="text" spellcheck="false"></label>` +
    `<p class="relay-command-error" role="alert"></p>` +
    `<p class="connection-menu-note">不支持在跳板机上输入口令。命令以 topology.json 为准，这里只按同一白名单检查写法。</p>` +
    `</div>`
  );
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
