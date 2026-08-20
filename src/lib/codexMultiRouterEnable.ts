import type { Provider } from "@/types";
import type { SwitchResult } from "@/lib/api/providers";

export type ProviderSwitchOutcome =
  | { ok: true; result: SwitchResult }
  | { ok: false; error: Error };

export async function enableCodexMultiRouterPlan(
  provider: Provider,
  switchProvider: (provider: Provider) => Promise<ProviderSwitchOutcome>,
): Promise<SwitchResult> {
  const routing = provider.settingsConfig?.codexRouting as
    | { schemaVersion?: number }
    | undefined;
  if (routing?.schemaVersion !== 2) {
    throw new Error("codex_multirouter_migration_required");
  }
  const outcome = await switchProvider(provider);
  if (!outcome.ok) {
    throw outcome.error;
  }
  return outcome.result;
}
