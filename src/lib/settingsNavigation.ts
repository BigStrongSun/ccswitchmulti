export const OPEN_SETTINGS_TAB_EVENT = "cc-switch:open-settings-tab";

export function openSettingsTab(tab: string): void {
  window.dispatchEvent(
    new CustomEvent(OPEN_SETTINGS_TAB_EVENT, { detail: { tab } }),
  );
}
