import { invoke } from "@tauri-apps/api/core";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export async function checkForUpdate(): Promise<
  { status: "up-to-date" } | { status: "available"; info: UpdateInfo }
> {
  const update = await invoke<UpdateInfo | null>("check_app_update");

  if (!update) {
    return { status: "up-to-date" };
  }

  return { status: "available", info: update };
}
