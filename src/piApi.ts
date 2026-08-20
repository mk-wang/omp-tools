import { invoke } from "@tauri-apps/api/core";
import type {
  CustomProvider,
  CustomProviderDiscoveryRequest,
  CustomProviderModel,
  CustomProviderSaveRequest,
  PiSnapshot,
  PiTiersFile
} from "./types";
export function loadPiSnapshot(piPath?: string): Promise<PiSnapshot> {
  return invoke<PiSnapshot>("load_pi_snapshot", { piPath: piPath || null });
}

export function savePiTiers(
  tiers: PiTiersFile,
  shadowedProviders: string[],
  piPath?: string
): Promise<PiSnapshot> {
  return invoke<PiSnapshot>("save_pi_tiers", { tiers, shadowedProviders, piPath: piPath || null });
}

export function savePiCustomProviders(
  request: CustomProviderSaveRequest
): Promise<CustomProvider[]> {
  return invoke<CustomProvider[]>("save_custom_providers", {
    request: { ...request, target: "pi" }
  });
}

export function discoverPiCustomProviderModels(
  request: CustomProviderDiscoveryRequest
): Promise<CustomProviderModel[]> {
  return invoke<CustomProviderModel[]>("discover_custom_provider_models", {
    request: { ...request, target: "pi" }
  });
}

export function openPiModelsFile(): Promise<void> {
  return invoke<void>("open_pi_models_file");
}
