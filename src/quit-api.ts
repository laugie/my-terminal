import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** The backend held a window close or quit because sessions are open. */
export function onCloseRequested(handler: (sessions: number) => void): Promise<() => void> {
  return listen<{ sessions: number }>("close-requested", (event) => {
    handler(event.payload.sessions);
  });
}

export function closePromptShown(): Promise<void> {
  return invoke("close_prompt_shown");
}

export function quitConfirmed(): Promise<void> {
  return invoke("quit_confirmed");
}
