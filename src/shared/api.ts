import { invoke } from "@tauri-apps/api/core";
import type { AskResult, CaptureMode, CapturePayload, DisplayInfo, Settings, StorageInfo } from "./types";

export function getSettings() {
  return invoke<Settings>("get_settings");
}

export function saveSettings(settings: Settings) {
  return invoke<Settings>("save_settings", { settings });
}

export function getLatestCapture() {
  return invoke<CapturePayload | null>("get_latest_capture");
}

export function askClaire(query: string, includeSearch: boolean) {
  return invoke<AskResult>("ask_claire", { query, includeSearch });
}

export function newChat() {
  return invoke<void>("new_chat");
}

export function clearContext() {
  return invoke<void>("clear_context");
}

export function hideOverlay() {
  return invoke<void>("hide_overlay");
}

export function setWindowMode(expanded: boolean) {
  return invoke<void>("set_window_mode", { expanded });
}

export function fitOverlay(width: number, height: number) {
  return invoke<void>("fit_overlay", { width, height });
}

export function openStorageFolder() {
  return invoke<void>("open_storage_folder");
}

export function storageInfo() {
  return invoke<StorageInfo>("storage_info");
}

export function recapture() {
  return invoke<CapturePayload>("recapture");
}

export function listDisplays() {
  return invoke<DisplayInfo[]>("list_displays");
}

export function captureDisplays(ids: number[]) {
  return invoke<CapturePayload>("capture_displays", { ids });
}

export function setCaptureMode(mode: CaptureMode, displayIds: number[]) {
  return invoke<void>("set_capture_mode", { mode, displayIds });
}
