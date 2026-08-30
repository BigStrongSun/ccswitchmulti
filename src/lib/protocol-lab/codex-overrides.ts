import type { CodexProtocolOverride } from "@/types";

export type CodexProtocolChoiceInput = "follow_auto" | CodexProtocolOverride;

export function normalizeCodexPublicModelKey(model: string): string {
  return model.trim().toLowerCase();
}

export function migrateCodexProtocolOverrideKey(
  overrides: Record<string, CodexProtocolOverride> | undefined,
  previousModel: string,
  nextModel: string,
): Record<string, CodexProtocolOverride> {
  const normalized = normalizeOverrideMap(overrides);
  const previousKey = normalizeCodexPublicModelKey(previousModel);
  const nextKey = normalizeCodexPublicModelKey(nextModel);
  const value = previousKey ? normalized[previousKey] : undefined;
  if (previousKey) delete normalized[previousKey];
  if (value && nextKey) normalized[nextKey] = value;
  return normalized;
}

export function setCodexProtocolOverride(
  overrides: Record<string, CodexProtocolOverride> | undefined,
  model: string,
  choice: CodexProtocolChoiceInput,
): Record<string, CodexProtocolOverride> {
  const normalized = normalizeOverrideMap(overrides);
  const key = normalizeCodexPublicModelKey(model);
  if (!key) return normalized;
  if (choice === "follow_auto") delete normalized[key];
  else normalized[key] = choice;
  return normalized;
}

function normalizeOverrideMap(
  overrides: Record<string, CodexProtocolOverride> | undefined,
): Record<string, CodexProtocolOverride> {
  const normalized: Record<string, CodexProtocolOverride> = {};
  for (const [model, transport] of Object.entries(overrides ?? {})) {
    const key = normalizeCodexPublicModelKey(model);
    if (key) normalized[key] = transport;
  }
  return normalized;
}
