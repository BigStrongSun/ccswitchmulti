export interface CodexOfficialModelSpec {
  modelId: string;
  documentationUrl: string;
  verifiedAt: string;
  contextWindow: number;
  maxOutputTokens: number;
}

const OFFICIAL_MODEL_SPECS: Record<string, CodexOfficialModelSpec> = {
  "gpt-6-luna": {
    modelId: "gpt-6-luna",
    documentationUrl:
      "https://developers.openai.com/api/docs/models/gpt-6-luna",
    verifiedAt: "2026-10-19",
    contextWindow: 1_050_000,
    maxOutputTokens: 128_000,
  },
  "gpt-6-sol": {
    modelId: "gpt-6-sol",
    documentationUrl: "https://developers.openai.com/api/docs/models/gpt-6-sol",
    verifiedAt: "2026-10-19",
    contextWindow: 1_050_000,
    maxOutputTokens: 128_000,
  },
  "gpt-6.1-sol": {
    modelId: "gpt-6.1-sol",
    documentationUrl:
      "https://developers.openai.com/api/docs/models/gpt-6.1-sol",
    verifiedAt: "2026-10-19",
    contextWindow: 1_050_000,
    maxOutputTokens: 128_000,
  },
};

export function getCodexOfficialModelSpec(
  modelId: string,
): CodexOfficialModelSpec | undefined {
  const normalized = modelId.trim().toLowerCase();
  return Object.prototype.hasOwnProperty.call(OFFICIAL_MODEL_SPECS, normalized)
    ? OFFICIAL_MODEL_SPECS[normalized]
    : undefined;
}
