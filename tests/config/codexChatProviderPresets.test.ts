import { describe, expect, it } from "vitest";
import {
  codexProviderPresets,
  generateThirdPartyConfig,
} from "@/config/codexProviderPresets";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexTopLevelInt,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";

const expectedChatPresets = new Map<
  string,
  { baseUrl: string; contextWindows: Record<string, number> }
>([
  [
    "火山Agentplan",
    {
      baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
      contextWindows: { "ark-code-latest": 256000 },
    },
  ],
  [
    "BytePlus",
    {
      baseUrl: "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
      contextWindows: { "ark-code-latest": 256000 },
    },
  ],
  [
    "DeepSeek",
    {
      baseUrl: "https://api.deepseek.com",
      contextWindows: {
        "deepseek-v4-flash": 1000000,
        "deepseek-v4-pro": 1000000,
      },
    },
  ],
  [
    "Zhipu GLM",
    {
      baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
      contextWindows: { "glm-5.2": 200000 },
    },
  ],
  [
    "Zhipu GLM en",
    {
      baseUrl: "https://api.z.ai/api/coding/paas/v4",
      contextWindows: { "glm-5.2": 200000 },
    },
  ],
  [
    "Baidu Qianfan Coding Plan",
    {
      baseUrl: "https://qianfan.baidubce.com/v2/coding",
      contextWindows: { "qianfan-code-latest": 131072 },
    },
  ],
  [
    "Kimi",
    {
      baseUrl: "https://api.moonshot.cn/v1",
      contextWindows: { "kimi-k2.7-code": 262144 },
    },
  ],
  [
    "StepFun",
    {
      baseUrl: "https://api.stepfun.com/step_plan/v1",
      contextWindows: {
        "step-3.7-flash": 262144,
        "step-3.5-flash-2603": 262144,
        "step-3.5-flash": 262144,
      },
    },
  ],
  [
    "StepFun en",
    {
      baseUrl: "https://api.stepfun.ai/step_plan/v1",
      contextWindows: {
        "step-3.7-flash": 262144,
        "step-3.5-flash-2603": 262144,
        "step-3.5-flash": 262144,
      },
    },
  ],
  [
    "ModelScope",
    {
      baseUrl: "https://api-inference.modelscope.cn/v1",
      contextWindows: { "ZhipuAI/GLM-5.1": 200000 },
    },
  ],
  [
    "BaiLing",
    {
      baseUrl: "https://api.tbox.cn/api/llm/v1",
      contextWindows: { "Ling-2.6-1T": 262144 },
    },
  ],
  [
    "SiliconFlow",
    {
      baseUrl: "https://api.siliconflow.cn/v1",
      contextWindows: { "Pro/MiniMaxAI/MiniMax-M2.7": 200000 },
    },
  ],
  [
    "SiliconFlow en",
    {
      baseUrl: "https://api.siliconflow.com/v1",
      contextWindows: { "MiniMaxAI/MiniMax-M2.7": 200000 },
    },
  ],
  [
    "Novita AI",
    {
      baseUrl: "https://api.novita.ai/openai/v1",
      contextWindows: { "zai-org/glm-5.1": 202800 },
    },
  ],
  [
    "Nvidia",
    {
      baseUrl: "https://integrate.api.nvidia.com/v1",
      contextWindows: { "moonshotai/kimi-k2.5": 262144 },
    },
  ],
]);

describe("Codex Chat provider presets", () => {
  it("keeps generated third-party bearer configs independent from OpenAI OAuth", () => {
    const config = generateThirdPartyConfig(
      "Remote CCSwitch",
      "https://www.matrixminecraft.cn:24443/ccswitch/v1",
      "gpt-5.5",
    );

    expect(extractCodexWireApi(config)).toBe("responses");
    expect(config).not.toContain("requires_openai_auth");
  });

  it("marks migrated Chat Completions presets for local routing", () => {
    for (const [name, expected] of expectedChatPresets) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset, `${name} preset`).toBeDefined();
      expect(preset?.apiFormat).toBe("openai_chat");
      expect(extractCodexBaseUrl(preset?.config)).toBe(expected.baseUrl);
      expect(extractCodexWireApi(preset?.config)).toBe("responses");
      expect(preset?.endpointCandidates).toContain(expected.baseUrl);
      expect(preset?.modelCatalog?.length).toBeGreaterThan(0);
      expect(extractCodexModelName(preset?.config)).toBe(
        preset?.modelCatalog?.[0]?.model,
      );
      expect(
        Object.fromEntries(
          (preset?.modelCatalog ?? []).map((model) => [
            model.model,
            model.contextWindow,
          ]),
        ),
      ).toEqual(expected.contextWindows);
    }
  });

  it("declares DeepSeek Codex models as text-only", () => {
    const preset = codexProviderPresets.find(
      (item) => item.name === "DeepSeek",
    );

    expect(preset?.modelCatalog).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          model: "deepseek-v4-flash",
          inputModalities: ["text"],
          textOnly: true,
          supportsImage: false,
        }),
        expect.objectContaining({
          model: "deepseek-v4-pro",
          inputModalities: ["text"],
          textOnly: true,
          supportsImage: false,
        }),
      ]),
    );
  });

  it("declares GLM 5.2 thinking and reasoning effort support", () => {
    for (const name of ["Zhipu GLM", "Zhipu GLM en"]) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset?.modelCatalog).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            model: "glm-5.2",
            inputModalities: ["text"],
            textOnly: true,
            supportsImage: false,
          }),
        ]),
      );
      expect(preset?.codexChatReasoning).toMatchObject({
        supportsThinking: true,
        supportsEffort: true,
        thinkingParam: "thinking",
        effortParam: "reasoning_effort",
        effortValueMode: "deepseek",
        outputFormat: "reasoning_content",
      });
    }
  });

  it("uses native Responses API for migrated CN providers without local route mapping", () => {
    const nativeResponsesPresets = new Map<
      string,
      { contextWindows: Record<string, number> }
    >([
      [
        "DouBaoSeed",
        { contextWindows: { "doubao-seed-2-1-pro-260628": 262144 } },
      ],
      ["Bailian", { contextWindows: { "qwen3-coder-plus": 1048576 } }],
      ["Longcat", { contextWindows: { "LongCat-2.0-Preview": 1048576 } }],
      ["MiniMax", { contextWindows: { "MiniMax-M3": 1000000 } }],
      ["MiniMax en", { contextWindows: { "MiniMax-M3": 1000000 } }],
      [
        "Xiaomi MiMo",
        {
          contextWindows: {
            "mimo-v2.5-pro": 1048576,
            "mimo-v2.5": 1048576,
          },
        },
      ],
      [
        "Xiaomi MiMo Token Plan (China)",
        {
          contextWindows: {
            "mimo-v2.5-pro": 1048576,
            "mimo-v2.5": 1048576,
          },
        },
      ],
    ]);

    for (const [name, expected] of nativeResponsesPresets) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset, `${name} preset`).toBeDefined();
      expect(preset?.apiFormat).toBe("openai_responses");
      // 原生 Responses 预设现在带 modelCatalog，供直连路径生成
      // ~/.codex 的 model-catalogs.json；前端已按 apiFormat 解耦，带 catalog
      // 不再等同于强制开启本地路由映射。
      expect((preset?.modelCatalog ?? []).length).toBeGreaterThan(0);
      expect(
        Object.fromEntries(
          (preset?.modelCatalog ?? []).map((model) => [
            model.model,
            model.contextWindow,
          ]),
        ),
      ).toEqual(expected.contextWindows);
      // 原生（直连）不走 Chat 转换，因此不需要 codexChatReasoning。
      expect(preset?.codexChatReasoning).toBeUndefined();
    }
  });

  it("aligns Xiaomi MiMo native Responses presets with official Codex guidance", () => {
    const expectedMiMoPresets = new Map([
      ["Xiaomi MiMo", "https://api.xiaomimimo.com/v1"],
      [
        "Xiaomi MiMo Token Plan (China)",
        "https://token-plan-cn.xiaomimimo.com/v1",
      ],
    ]);

    for (const [name, baseUrl] of expectedMiMoPresets) {
      const preset = codexProviderPresets.find((item) => item.name === name);

      expect(preset, `${name} preset`).toBeDefined();
      expect(extractCodexBaseUrl(preset?.config)).toBe(baseUrl);
      expect(extractCodexWireApi(preset?.config)).toBe("responses");
      expect(extractCodexModelName(preset?.config)).toBe("mimo-v2.5-pro");
      expect(
        extractCodexTopLevelInt(preset?.config, "model_context_window"),
      ).toBe(1048576);
      expect(preset?.config).toContain(
        "model_supports_reasoning_summaries = true",
      );
      expect(preset?.config).toContain('model_reasoning_summary = "none"');
      expect(preset?.config).toContain('web_search = "disabled"');
      expect(preset?.apiFormat).toBe("openai_responses");
      expect(preset?.modelCatalog ?? []).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            model: "mimo-v2.5-pro",
            contextWindow: 1048576,
          }),
          expect.objectContaining({
            model: "mimo-v2.5",
            contextWindow: 1048576,
          }),
        ]),
      );
    }
  });
});
