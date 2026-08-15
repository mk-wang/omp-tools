import { invoke } from "@tauri-apps/api/core";
import type { ConfigSnapshot, SaveRequest } from "./types";

export function loadSnapshot(): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("load_snapshot");
}

export function saveSnapshot(request: SaveRequest): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("save_snapshot", { request });
}
