import { invoke } from "@tauri-apps/api/core";
import type {
  ConfigSnapshot,
  CustomProvider,
  CustomProviderDiscoveryRequest,
  CustomProviderModel,
  CustomProviderSaveRequest,
  SaveRequest
} from "./types";
export function loadSnapshot(ompPath?: string): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("load_snapshot", { ompPath: ompPath || null });
}

export function saveSnapshot(request: SaveRequest): Promise<ConfigSnapshot> {
  return invoke<ConfigSnapshot>("save_snapshot", { request });
}

export function saveCustomProviders(
  request: CustomProviderSaveRequest
): Promise<CustomProvider[]> {
  return invoke<CustomProvider[]>("save_custom_providers", { request });
}

export function discoverCustomProviderModels(
  request: CustomProviderDiscoveryRequest
): Promise<CustomProviderModel[]> {
  return invoke<CustomProviderModel[]>("discover_custom_provider_models", { request });
}

export type EditableConfigFile = "config" | "models";

export function openConfigFile(kind: EditableConfigFile): Promise<void> {
  return invoke<void>("open_config_file", { kind });
}
