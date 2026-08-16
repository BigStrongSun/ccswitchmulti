import type { Provider } from "@/types";
import type { SwitchResult } from "@/lib/api/providers";

export type ProviderSwitchOutcome =
  | { ok: true; result: SwitchResult }
  | { ok: false; error: Error };

export async function enableCodexMultiRouterPlan(
  provider: Provider,
  switchProvider: (provider: Provider) => Promise<ProviderSwitchOutcome>,
): Promise<SwitchResult> {
  const outcome = await switchProvider(provider);
  if (!outcome.ok) {
    throw outcome.error;
  }
  return outcome.result;
}
