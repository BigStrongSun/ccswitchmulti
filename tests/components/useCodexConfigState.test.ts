import { describe, expect, it } from "vitest";
import { extractCodexRoutingConfig } from "@/components/providers/forms/hooks/useCodexConfigState";

describe("extractCodexRoutingConfig", () => {
  it("migrates legacy array codexRouting into the object schema", () => {
    const routing = extractCodexRoutingConfig({
      codexRouting: [
        {
          id: "router-codex-official",
          name: "OpenAI Official",
          providerId: "codex-official",
          models: ["gpt-5.5"],
        },
        {
          id: "router-deepseek",
          label: "DeepSeek",
          provider_id: "codex-deepseek",
          model_prefixes: ["deepseek-"],
        },
      ],
    });

    expect(routing.enabled).toBe(true);
    const routes = routing.routes ?? [];
    expect(routes).toHaveLength(2);
    expect(routes[0].id).toBe("router-codex-official");
    expect(routes[0].match.models).toEqual(["gpt-5.5"]);
    expect(routes[1].match.prefixes).toEqual(["deepseek-"]);
  });
});
