# 2026-09-11 GLM through CCSM has no visible reasoning

- The active provider in `C:\Users\sunda\.cc-switch\cc-switch.db` is `Zhipu GLM` (`4f9dea17-b9fd-4aa8-ab90-2222e77b8818`). It is still the older Coding Plan Chat configuration: base URL `https://open.bigmodel.cn/api/coding/paas/v4`, `apiFormat=openai_chat`, and `codexProtocolMode=manual`.
- Its persisted `codexChatReasoning` says `supportsThinking=true`, `thinkingParam=thinking`, `effortParam=reasoning_effort`, and `outputFormat=reasoning_content`, but the provider meta has no `codexReasoningProjection`. The model catalog rows for `glm-5.3` and `glm-5.3-flash` also have no schema-v2 `reasoning` object.
- `resolve_codex_chat_reasoning_projection()` intentionally fail-closes manual protocol mode: it emits `RawReasoningText` only for the exact `codexReasoningProjection=raw_reasoning_text`; any missing/other value becomes `ReasoningProjection::None`. The streaming converter then accumulates Chat `reasoning_content` but emits no Responses reasoning SSE when the projection is `None`.
- Runtime logs prove the route is `/responses` externally but converts to `/chat/completions` at `https://open.bigmodel.cn/api/coding/paas/v4/chat/completions`. The protocol profile for GLM is independently verified as Chat, with readable reasoning from `choices[].message/delta.reasoning_content`; therefore the upstream did return reasoning and the loss is at CCSM's projection policy boundary.
- Logs also warn that GLM catalog reasoning declarations are ignored because `disableAllowed` is missing. This is a stale/incomplete persisted catalog issue and explains the warning, but it is not the direct visibility blocker while the explicit provider-level Chat reasoning config remains present.
- The current source preset has already moved Zhipu GLM to native Responses (`https://open.bigmodel.cn/api/v1`) and includes complete schema-v2 reasoning metadata. The active database provider has not been migrated to that preset, so source and runtime configuration are not the same.

## 修复状态

- `resolve_codex_chat_reasoning_projection()` now preserves the explicit
  `codexReasoningProjection=raw_reasoning_text` behavior and, when that field is
  absent, derives `RawReasoningText` only from an explicit
  `codexChatReasoning.outputFormat=reasoning_content` declaration.
- Manual providers without a reasoning-content declaration still fail closed;
  `reasoning_summary` is never synthesized from manual metadata and still
  requires verified Responses protocol evidence.
- Regression coverage is green: the legacy GLM Chat shape now resolves to raw
  reasoning, the anti-summary safety test passes, and all 136 `codex` provider
  unit tests pass.

## Safe recovery boundary

Do not fix this by only adding `disableAllowed` to the catalog. For the currently active Chat route, either persist the exact raw projection (`codexReasoningProjection=raw_reasoning_text`) or move the provider out of manual protocol mode and let a verified, target-bound Chat profile select the projection. A separate migration is needed if the intent is to use the current native Responses preset.
