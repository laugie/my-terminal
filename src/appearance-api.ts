import { invoke } from "@tauri-apps/api/core";
import type { AppearanceChange, AppearanceView } from "./appearance.ts";

export function loadAppearance(): Promise<AppearanceView> {
  return invoke<AppearanceView>("appearance_load");
}

/** Saves only the fields that are set. */
export function saveAppearance(change: AppearanceChange): Promise<AppearanceView> {
  return invoke<AppearanceView>("appearance_set", {
    theme: change.theme ?? null,
    font: change.font ?? null,
  });
}

/**
 * Wait for the bundled font so xterm measures its cells with it, not with
 * the fallback. Other fonts are local and need no download.
 */
export async function fontReady(family: string, bundled: boolean): Promise<void> {
  if (!bundled || !document.fonts) {
    return;
  }
  try {
    await Promise.all([
      document.fonts.load(`400 14px "${family}"`),
      document.fonts.load(`700 14px "${family}"`),
    ]);
  } catch {
    // The stack falls back to a system font.
  }
}

/**
 * Whether a local font exists: text set in it measures differently from the
 * generic fallbacks. Bundled fonts are always present.
 */
export function fontInstalled(family: string): boolean {
  if (family === "") {
    return true;
  }
  const context = document.createElement("canvas").getContext("2d");
  if (!context) {
    return true;
  }
  const sample = "mmmmmmmmmmlli10OO@#";
  for (const generic of ["monospace", "serif", "sans-serif"]) {
    context.font = `72px ${generic}`;
    const base = context.measureText(sample).width;
    context.font = `72px "${family}", ${generic}`;
    if (context.measureText(sample).width !== base) {
      return true;
    }
  }
  return false;
}
