import { invoke } from "@tauri-apps/api/core";

/** System clipboard text through the app, not `navigator.clipboard`. Empty when it holds no text. */
export function readClipboardText(): Promise<string> {
  return invoke<string>("clipboard_read_text");
}

export function writeClipboardText(text: string): Promise<void> {
  return invoke("clipboard_write_text", { text });
}
