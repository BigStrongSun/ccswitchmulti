export type OpenCodeGoProtocol = "responses" | "messages" | "chat";

export type OpenCodeGoInputModality =
  | "text"
  | "image"
  | "audio"
  | "video"
  | "pdf";

export type OpenCodeGoReasoningEffort =
  | "none"
  | "minimal"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max";

export interface OpenCodeGoModel {
  id: string;
  name: string;
  protocol: OpenCodeGoProtocol;
  context: number;
  output: number;
  input: readonly OpenCodeGoInputModality[];
  reasoning:
    | { kind: "unknown" }
    | { kind: "toggle" }
    | {
        kind: "effort";
        efforts: readonly OpenCodeGoReasoningEffort[];
        disableAllowed?: boolean;
      };
}

/**
 * Maintained OpenCode Go baseline.
 *
 * Protocol ownership follows the first-party Go endpoint table. Limits and
 * modalities are the provider-scoped models.dev values reviewed on
 * 2026-09-08. Deprecated compatibility aliases returned by `/models` are
 * deliberately absent; dynamic model discovery can still expose them.
 */
export const OPEN_CODE_GO_MODELS = [
  {
    id: "grok-4.6",
    name: "Grok 4.6",
    protocol: "responses",
    context: 500_000,
    output: 500_000,
    input: ["text", "image"],
    reasoning: { kind: "effort", efforts: ["low", "medium", "high", "xhigh"] },
  },
  {
    id: "gpt-5.6-luna",
    name: "GPT-5.6 Luna",
    protocol: "responses",
    context: 1_050_000,
    output: 128_000,
    input: ["text", "image", "pdf"],
    reasoning: {
      kind: "effort",
      efforts: ["low", "medium", "high", "xhigh", "max"],
      disableAllowed: true,
    },
  },
  {
    id: "muse-spark-1.3-contributor",
    name: "Muse Spark 1.3 Contributor",
    protocol: "responses",
    context: 1_048_576,
    output: 131_072,
    input: ["text", "image", "video", "pdf", "audio"],
    reasoning: {
      kind: "effort",
      efforts: ["minimal", "low", "medium", "high", "xhigh"],
    },
  },
  {
    id: "muse-spark-1.2-contributor",
    name: "Muse Spark 1.2 Contributor",
    protocol: "responses",
    context: 1_048_576,
    output: 131_072,
    input: ["text", "image", "video", "pdf", "audio"],
    reasoning: {
      kind: "effort",
      efforts: ["minimal", "low", "medium", "high", "xhigh"],
    },
  },
  {
    id: "minimax-m3",
    name: "MiniMax-M3",
    protocol: "messages",
    context: 1_000_000,
    output: 131_072,
    input: ["text", "image", "video"],
    reasoning: { kind: "toggle" },
  },
  {
    id: "minimax-m2.7",
    name: "MiniMax-M2.7",
    protocol: "messages",
    context: 204_800,
    output: 131_072,
    input: ["text"],
    reasoning: { kind: "unknown" },
  },
  {
    id: "qwen3.8-max",
    name: "Qwen3.8 Max",
    protocol: "messages",
    context: 1_000_000,
    output: 131_072,
    input: ["text", "image", "video"],
    reasoning: {
      kind: "effort",
      efforts: ["low", "medium", "xhigh"],
      disableAllowed: true,
    },
  },
  {
    id: "qwen3.8-flash",
    name: "Qwen3.8 Flash",
    protocol: "messages",
    context: 1_000_000,
    output: 131_072,
    input: ["text", "image", "video"],
    reasoning: {
      kind: "effort",
      efforts: ["low", "medium", "xhigh"],
      disableAllowed: true,
    },
  },
  {
    id: "qwen3.7-max",
    name: "Qwen3.7 Max",
    protocol: "messages",
    context: 1_000_000,
    output: 65_536,
    input: ["text"],
    reasoning: { kind: "toggle" },
  },
  {
    id: "qwen3.7-plus",
    name: "Qwen3.7 Plus",
    protocol: "messages",
    context: 1_000_000,
    output: 65_536,
    input: ["text", "image", "video"],
    reasoning: { kind: "toggle" },
  },
  {
    id: "qwen3.6-plus",
    name: "Qwen3.6 Plus",
    protocol: "messages",
    context: 1_000_000,
    output: 65_536,
    input: ["text", "image", "video"],
    reasoning: { kind: "toggle" },
  },
  {
    id: "glm-5.3-flash",
    name: "GLM-5.3-Flash (2x usage)",
    protocol: "chat",
    context: 1_000_000,
    output: 131_072,
    input: ["text", "image", "video", "pdf"],
    reasoning: { kind: "effort", efforts: ["low", "high", "max"] },
  },
  {
    id: "glm-5.3",
    name: "GLM-5.3",
    protocol: "chat",
    context: 1_000_000,
    output: 131_072,
    input: ["text"],
    reasoning: { kind: "effort", efforts: ["low", "high", "max"] },
  },
  {
    id: "glm-5.2",
    name: "GLM-5.2",
    protocol: "chat",
    context: 1_000_000,
    output: 131_072,
    input: ["text"],
    reasoning: { kind: "effort", efforts: ["high", "max"] },
  },
  {
    id: "glm-5.1",
    name: "GLM-5.1",
    protocol: "chat",
    context: 202_752,
    output: 32_768,
    input: ["text"],
    reasoning: { kind: "unknown" },
  },
  {
    id: "kimi-k3",
    name: "Kimi K3",
    protocol: "chat",
    context: 1_048_576,
    output: 131_072,
    input: ["text", "image", "video"],
    reasoning: { kind: "effort", efforts: ["max"] },
  },
  {
    id: "kimi-k2.7-code",
    name: "Kimi K2.7 Code",
    protocol: "chat",
    context: 262_144,
    output: 262_144,
    input: ["text", "image", "video"],
    reasoning: { kind: "unknown" },
  },
  {
    id: "kimi-k2.6",
    name: "Kimi K2.6",
    protocol: "chat",
    context: 262_144,
    output: 65_536,
    input: ["text", "image", "video"],
    reasoning: { kind: "unknown" },
  },
  {
    id: "longcat-2.0",
    name: "LongCat-2.0",
    protocol: "chat",
    context: 1_000_000,
    output: 131_072,
    input: ["text"],
    reasoning: { kind: "toggle" },
  },
  {
    id: "deepseek-v4-pro",
    name: "DeepSeek V4 Pro",
    protocol: "chat",
    context: 1_000_000,
    output: 384_000,
    input: ["text"],
    reasoning: { kind: "effort", efforts: ["high", "max"] },
  },
  {
    id: "deepseek-v4-flash",
    name: "DeepSeek V4 Flash",
    protocol: "chat",
    context: 1_000_000,
    output: 384_000,
    input: ["text"],
    reasoning: { kind: "effort", efforts: ["low", "high", "max"] },
  },
  {
    id: "deepseek-v4-flash-vision-exp",
    name: "DeepSeek V4 Flash Vision Exp",
    protocol: "chat",
    context: 1_000_000,
    output: 384_000,
    input: ["text", "image"],
    reasoning: {
      kind: "effort",
      efforts: ["low", "high", "max"],
      disableAllowed: true,
    },
  },
  {
    id: "mimo-v2.5",
    name: "MiMo V2.5",
    protocol: "chat",
    context: 1_000_000,
    output: 128_000,
    input: ["text", "image", "audio", "video"],
    reasoning: { kind: "unknown" },
  },
  {
    id: "mimo-v2.5-pro",
    name: "MiMo V2.5 Pro",
    protocol: "chat",
    context: 1_048_576,
    output: 128_000,
    input: ["text"],
    reasoning: { kind: "unknown" },
  },
  {
    id: "hy4-preview",
    name: "Hy4 preview",
    protocol: "chat",
    context: 1_024_000,
    output: 64_000,
    input: ["text"],
    reasoning: {
      kind: "effort",
      efforts: ["high"],
      disableAllowed: true,
    },
  },
  {
    id: "hy3",
    name: "Hy3",
    protocol: "chat",
    context: 256_000,
    output: 128_000,
    input: ["text"],
    reasoning: {
      kind: "effort",
      efforts: ["low", "high"],
      disableAllowed: true,
    },
  },
  {
    id: "omen-alpha",
    name: "Omen Alpha",
    protocol: "chat",
    context: 500_000,
    output: 128_000,
    input: ["text", "image"],
    reasoning: { kind: "effort", efforts: ["low", "high"] },
  },
] as const satisfies readonly OpenCodeGoModel[];

export type OpenCodeGoModelId = (typeof OPEN_CODE_GO_MODELS)[number]["id"];

export function openCodeGoModelsFor(protocol: OpenCodeGoProtocol) {
  return OPEN_CODE_GO_MODELS.filter((model) => model.protocol === protocol);
}
