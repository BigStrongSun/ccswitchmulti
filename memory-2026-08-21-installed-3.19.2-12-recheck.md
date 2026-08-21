# 2026-08-21 安装态 3.19.2-12 hosted search 复核

- 当前运行进程是 `C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe`，
  Product/FileVersion 为 `3.19.2-12`，监听 `127.0.0.1:15721`，health 返回 HTTP 200。
- 当前二进制和 `codex-router.log` 中均未发现旧错误
  `Mixed hosted and ordinary function tool calls are not supported in one streaming turn`。
  Codex 历史中能找到的同一错误发生在 2026-08-21 11:57 UTC，早于本次 3.19.2-12
  安装（本地 20:22）。
- 当前安装态真实 canary：
  - `deepseek-v4-pro`：HTTP 200，收到 `response.web_search_call.in_progress/searching/completed`，
    返回 `CCSM_THIRD_PARTY_HOSTED_SEARCH_OK`。
  - `qwen3.8`：HTTP 200，日志确认 `responses_to_chat=true`、
    `hosted_tools=[web_search]` 和 `hosted_tool_choice=object(keys=[function,type])`，
    但只收到 reasoning/完成事件，没有任何 function call，因此记录
    `hosted_tool_not_called`。
- `verify_qwen38_streaming.py` 的普通文本、普通 function tool、原始 replay 均通过。
- 结论：混合 hosted/function 的 CCSM 旧逻辑已在当前安装态生效；若用户仍看到截图中的
  旧错误，应先确认请求是否来自本次安装后的新 Codex turn。Qwen3.8 的搜索未调用是上游
  vLLM/模型没有发出 function call，不是 CCSM hosted loop 当前的混合调用错误。
