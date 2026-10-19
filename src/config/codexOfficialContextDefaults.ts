import contextDefaults from "./codexOfficialContextDefaults.json";

export interface CodexOfficialContextDefault {
  model: string;
  contextWindow: number;
  source: string;
  sourceDate: string;
  documentationUrl: string;
}

const OFFICIAL_CONTEXT_DEFAULTS = new Map<string, CodexOfficialContextDefault>(
  contextDefaults.models.map((item) => [
    item.model.toLowerCase(),
    {
      ...item,
      source: contextDefaults.source,
      sourceDate: contextDefaults.sourceDate,
      documentationUrl: contextDefaults.documentationUrl,
    },
  ]),
);

export function getCodexOfficialContextDefault(
  modelId: string,
): CodexOfficialContextDefault | undefined {
  return OFFICIAL_CONTEXT_DEFAULTS.get(modelId.trim().toLowerCase());
}
