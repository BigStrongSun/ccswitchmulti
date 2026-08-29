import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  CodexHistoryReplay,
  CodexToolSchemaDialect,
  Provider,
  UniversalProvider,
} from "@/types";
import type { CodexRoutingProjectionStatus } from "./providers";

export type CodexProtocolTransport = "open_ai_responses" | "open_ai_chat";
export type CodexProtocolProbeStage =
  | "baseline"
  | "streaming"
  | "reasoning"
  | "forced_tool"
  | "continuation";
export type CodexProtocolProbeStageStatus =
  | "passed"
  | "unsupported"
  | "failed"
  | "skipped";
export type CodexProtocolProbeReadiness = "verified" | "partial" | "unverified";
export type CodexReasoningSemantic = "readable" | "summary" | "opaque" | "none";
export type CodexReasoningSource =
  | "reasoning_content"
  | "reasoning"
  | "reasoning_details"
  | "think_tags"
  | "native_responses"
  | "none";

export type CodexProtocolProbeFailureKind =
  | "http_status"
  | "timeout"
  | "network"
  | "response_too_large"
  | "invalid_response"
  | "invalid_request";

export interface CodexProtocolProbeFailure {
  stage: CodexProtocolProbeStage;
  kind: CodexProtocolProbeFailureKind;
  status_code?: number;
}

export type CodexProtocolProbeProgressEvent =
  | { kind: "candidate_started"; model: string }
  | {
      kind: "stage_started";
      model: string;
      transport: CodexProtocolTransport;
      stage: CodexProtocolProbeStage;
    }
  | {
      kind: "stage_finished";
      model: string;
      transport: CodexProtocolTransport;
      stage: CodexProtocolProbeStage;
      stageStatus: CodexProtocolProbeStageStatus;
      failure?: CodexProtocolProbeFailure;
    }
  | {
      kind: "reasoning_classified";
      model: string;
      transport: CodexProtocolTransport;
      stage: "reasoning";
      reasoningSemantic: CodexReasoningSemantic;
      reasoningSource: CodexReasoningSource;
    }
  | {
      kind: "branch_finished";
      model: string;
      transport: CodexProtocolTransport;
      readiness: CodexProtocolProbeReadiness;
    }
  | {
      kind: "candidate_finished";
      model: string;
      selectedTransport: CodexProtocolTransport | null;
      readiness: CodexProtocolProbeReadiness;
    }
  | {
      kind: "batch_finished";
      total: number;
      verified: number;
      partial: number;
      failed: number;
    };

export interface CodexProtocolProbeBranch {
  assessment: {
    transport: CodexProtocolTransport;
    baseline: CodexProtocolProbeStageStatus;
    streaming: CodexProtocolProbeStageStatus;
    forced_tool: CodexProtocolProbeStageStatus;
    continuation: CodexProtocolProbeStageStatus;
  };
  reasoning_shape: {
    semantic: CodexReasoningSemantic;
    source: CodexReasoningSource;
    pre_tool_visible_content: "absent" | "present";
  };
  tool_schema_dialect?: CodexToolSchemaDialect;
  history_replay?: CodexHistoryReplay;
  failures?: CodexProtocolProbeFailure[];
}

export interface CodexProtocolCompatibilityRecord {
  probeVersion: number;
  target: {
    provider_id: string;
    route_id: string | null;
    public_model: string;
    upstream_model: string;
    transport: CodexProtocolTransport;
    endpoint_fingerprint: string;
    authentication_kind: string;
    credential_fingerprint: string;
    request_policy_fingerprint: string;
  };
  result: {
    selected_transport: CodexProtocolTransport | null;
    readiness: CodexProtocolProbeReadiness;
    branches: CodexProtocolProbeBranch[];
  };
  testedAt: number;
  expiresAt: number;
}

export interface CodexProviderProtocolPreflightOutcome {
  provider: Provider;
  records: CodexProtocolCompatibilityRecord[];
  observations: CodexProtocolCompatibilityRecord[];
  receiptIds: string[];
  protocolApplied: boolean;
}

export interface CodexProviderSetBlockedModel {
  model: string;
  upstreamModel: string;
  reason: string;
}

export type CodexProviderSetPlan =
  | {
      kind: "single";
      transport: CodexProtocolTransport;
    }
  | {
      kind: "split";
      responses_provider_id: string;
      chat_provider_id: string;
    }
  | {
      kind: "blocked";
      models: CodexProviderSetBlockedModel[];
    };

export interface CodexProviderSetPreview {
  digest: string;
  sourceProviderId: string;
  responsesModels: string[];
  chatModels: string[];
  plan: CodexProviderSetPlan;
}

export type CodexProviderSetCommitIntent =
  | "accept_single"
  | "confirm_split"
  | "confirm_manual";

export interface CodexProviderSetCommitOutcome {
  preview: CodexProviderSetPreview;
  projections: CodexRoutingProjectionStatus[];
  status: "committed" | "committed_with_projection_error";
  projectionErrorCode?: string | null;
}

export interface CodexProviderSetBatchSource {
  provider: Provider;
  receiptIds: string[];
}

export interface CodexProviderSetBatchPreview {
  digest: string;
  sourcePreviews: CodexProviderSetPreview[];
  routerProviderId: string;
  requiresSplitConfirmation: boolean;
  blocked: boolean;
}

export interface CodexProviderSetBatchCommitOutcome {
  preview: CodexProviderSetBatchPreview;
  router: Provider;
  projections: CodexRoutingProjectionStatus[];
  status: "committed" | "committed_with_projection_error";
  projectionErrorCode?: string | null;
}

export interface UniversalProviderSetPreview {
  digest: string;
  universalProviderId: string;
  codex: CodexProviderSetPreview | null;
}

export interface UniversalProviderSetCommitOutcome {
  preview: UniversalProviderSetPreview;
}

export async function prepareCodexProviderSet(
  provider: Provider,
  receiptIds: string[],
): Promise<CodexProviderSetPreview> {
  return invoke<CodexProviderSetPreview>("prepare_codex_provider_set", {
    request: { provider, receiptIds },
  });
}

export async function commitCodexProviderSet(
  provider: Provider,
  receiptIds: string[],
  digest: string,
  intent: CodexProviderSetCommitIntent,
): Promise<CodexProviderSetCommitOutcome> {
  return invoke<CodexProviderSetCommitOutcome>("commit_codex_provider_set", {
    request: { provider, receiptIds, digest, intent },
  });
}

export async function prepareCodexProviderSetBatch(
  sources: CodexProviderSetBatchSource[],
  router: Provider,
): Promise<CodexProviderSetBatchPreview> {
  return invoke<CodexProviderSetBatchPreview>(
    "prepare_codex_provider_set_batch",
    { request: { sources, router } },
  );
}

export async function commitCodexProviderSetBatch(
  sources: CodexProviderSetBatchSource[],
  router: Provider,
  digest: string,
  intent: CodexProviderSetCommitIntent,
): Promise<CodexProviderSetBatchCommitOutcome> {
  return invoke<CodexProviderSetBatchCommitOutcome>(
    "commit_codex_provider_set_batch",
    { request: { sources, router, digest, intent } },
  );
}

export async function preflightCodexProviderProtocolCompatibility(
  provider: Provider,
  onProgress: (event: CodexProtocolProbeProgressEvent) => void,
): Promise<CodexProviderProtocolPreflightOutcome> {
  const onEvent = new Channel<CodexProtocolProbeProgressEvent>();
  onEvent.onmessage = onProgress;
  return invoke<CodexProviderProtocolPreflightOutcome>(
    "preflight_codex_provider_protocol_compatibility",
    { provider, onEvent },
  );
}

export async function preflightUniversalCodexProtocolCompatibility(
  provider: UniversalProvider,
  onProgress: (event: CodexProtocolProbeProgressEvent) => void,
): Promise<CodexProviderProtocolPreflightOutcome | null> {
  const onEvent = new Channel<CodexProtocolProbeProgressEvent>();
  onEvent.onmessage = onProgress;
  return invoke<CodexProviderProtocolPreflightOutcome | null>(
    "preflight_universal_codex_protocol_compatibility",
    { provider, onEvent },
  );
}

export async function listCodexProtocolProbeObservations(
  providerId: string,
): Promise<CodexProtocolCompatibilityRecord[]> {
  return invoke<CodexProtocolCompatibilityRecord[]>(
    "list_codex_protocol_probe_observations",
    { providerId },
  );
}

export async function prepareUniversalProviderSet(
  provider: UniversalProvider,
  receiptIds: string[],
): Promise<UniversalProviderSetPreview> {
  return invoke<UniversalProviderSetPreview>("prepare_universal_provider_set", {
    request: { provider, receiptIds },
  });
}

export async function commitUniversalProviderSet(
  provider: UniversalProvider,
  receiptIds: string[],
  digest: string,
  intent: CodexProviderSetCommitIntent,
): Promise<UniversalProviderSetCommitOutcome> {
  return invoke<UniversalProviderSetCommitOutcome>(
    "commit_universal_provider_set",
    { request: { provider, receiptIds, digest, intent } },
  );
}
