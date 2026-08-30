export type WizardPageKey =
  | "welcome"
  | "inventory"
  | "first-provider"
  | "readiness"
  | "sources"
  | "catalog"
  | "protocol"
  | "models"
  | "model-order"
  | "reasoning"
  | "subagents-tools"
  | "routing-review"
  | "save-enable"
  | "acceptance";

export type WizardAcceptanceStatus = "waiting" | "passed" | "failed";

export interface WizardFlowContext {
  providerCount: number;
  selectedSourceCount: number;
  allSelectedSourcesReady: boolean;
  catalogPrepared: boolean;
  protocolProbeComplete: boolean;
  hasVisibleModels: boolean;
  planSaved: boolean;
  planEnabled: boolean;
  acceptanceStatus: WizardAcceptanceStatus;
}

const BEFORE_SOURCE_PAGES: WizardPageKey[] = ["welcome", "inventory"];
const AFTER_SOURCE_PAGES: WizardPageKey[] = [
  "readiness",
  "sources",
  "catalog",
  "protocol",
  "models",
  "model-order",
  "reasoning",
  "subagents-tools",
  "routing-review",
  "save-enable",
  "acceptance",
];

export function buildWizardPageSequence(
  context: WizardFlowContext,
): WizardPageKey[] {
  return context.providerCount === 0
    ? [...BEFORE_SOURCE_PAGES, "first-provider", ...AFTER_SOURCE_PAGES]
    : [...BEFORE_SOURCE_PAGES, ...AFTER_SOURCE_PAGES];
}

export function canEnterWizardPage(
  page: WizardPageKey,
  context: WizardFlowContext,
): boolean {
  switch (page) {
    case "welcome":
    case "inventory":
      return true;
    case "first-provider":
      return context.providerCount === 0;
    case "readiness":
    case "sources":
      return context.providerCount > 0;
    case "catalog":
      return context.selectedSourceCount > 0 && context.allSelectedSourcesReady;
    case "protocol":
      return context.selectedSourceCount > 0 && context.catalogPrepared;
    case "models":
    case "model-order":
    case "reasoning":
    case "subagents-tools":
      return context.protocolProbeComplete && context.hasVisibleModels;
    case "routing-review":
    case "save-enable":
      return (
        context.protocolProbeComplete &&
        context.hasVisibleModels &&
        context.selectedSourceCount > 0
      );
    case "acceptance":
      return context.planSaved && context.planEnabled;
    default:
      return false;
  }
}

export function requiredWizardPrerequisite(
  page: WizardPageKey,
  context: WizardFlowContext,
): WizardPageKey | null {
  if (canEnterWizardPage(page, context)) return null;

  switch (page) {
    case "readiness":
    case "sources":
      return "first-provider";
    case "catalog":
      return "sources";
    case "protocol":
      return context.selectedSourceCount > 0 ? "catalog" : "sources";
    case "models":
    case "model-order":
    case "reasoning":
    case "subagents-tools":
    case "routing-review":
    case "save-enable":
      if (context.selectedSourceCount === 0) return "sources";
      if (!context.allSelectedSourcesReady) return "sources";
      if (!context.catalogPrepared || !context.hasVisibleModels)
        return "catalog";
      return "protocol";
    case "acceptance":
      return "save-enable";
    case "first-provider":
      return "readiness";
    case "welcome":
    case "inventory":
    default:
      return null;
  }
}
