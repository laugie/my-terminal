import { invoke } from "@tauri-apps/api/core";
import type { ZoomView } from "./zoom.ts";

/** Applies the stored zoom to this window and returns it. */
export function loadUiZoom(): Promise<ZoomView> {
  return invoke<ZoomView>("ui_zoom_load");
}

/** Applies `percent` to this window, then saves it. */
export function saveUiZoom(percent: number): Promise<ZoomView> {
  return invoke<ZoomView>("ui_zoom_set", { percent });
}
