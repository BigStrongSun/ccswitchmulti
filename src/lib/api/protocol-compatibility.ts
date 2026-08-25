import { Channel, invoke } from "@tauri-apps/api/core";

import type { Provider } from "@/types";

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
  protocolApplied: boolean;
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
