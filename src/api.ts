import { invoke } from "@tauri-apps/api/core";
import type { ConfigSnapshot, SaveRequest } from "./types";

export function loadSnapshot(ompPath?: string): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("load_snapshot", { ompPath: ompPath || null });
}

export function saveSnapshot(request: SaveRequest): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("save_snapshot", { request });
}
