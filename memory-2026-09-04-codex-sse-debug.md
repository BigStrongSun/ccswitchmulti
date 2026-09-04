# Codex SSE 诊断与重试边界

## 本次故障证据

目标任务 `01a05b75-d141-7652-ac6c-a6258653316d` 在 22:00、22:01 和 22:59 再现同一上游 SSE `error`，类型 `invalid_request_error`，消息摘要 `540a484c4d9c46e7`。请求模型为 Sol，目标为 OpenAI 官方 Codex Responses。不能从摘要判断具体错误正文、触发内容或策略类别。

Codex 运行日志直接确认每轮是首次请求加五次客户端重试。CCSM 原生流日志的 attempt=0 不是代理重试失效：上游已明确返回错误终态，不是网络传输中断。既有代理断网恢复逻辑自 2026-08-24 后在该文件中未再变动，安装 v29 对应源码与本次基线一致。

旧实现的问题有三层：通用 error 原样传给没有相应分支的 Codex 解析器，真实原因退化成缺少 response.completed；默认 router 日志刻意只记录摘要，没有可按任务启用的错误字段采集；usage 过滤器不收集错误终态，记账记录因此只保留 HTTP 200 而没有流失败信息。

## 实现边界

- 原始上游 error / response.error 转为 response.failed，保留错误 type/code/message/param；已有 response.failed 原样转发。没有推断或伪造 invalid_prompt 等错误码，没有生成成功终态，没有改变政策判决。客户端仍根据真实错误码决定是否重试，未知或可重试错误不能保证只请求一次。
- 网络恢复仍保留首个实质内容前的有限重连、退避、心跳；已输出正文或工具后不在代理重放请求，避免重复执行。客户端可恢复自己的轮次，不能把“代理不重放”理解成整个系统不重试。
- Codex 用量采集接收错误终态；日志用 502 表示代理流失败，并在安全错误摘要中区分上游 HTTP 200。这个 502 是请求日志的语义结果，不是改写已经发出的 HTTP 响应头。上游错误正文不进入普通请求日志；已报告用量继续保留，未报告用量的零值不表示免费。
- 详细采集默认关闭。热读取应用配置目录的 codex-error-capture.json，只允许一个指定 UUID 任务、最多未来 30 分钟、1–20 条错误记录。删除控制文件即可关闭；到期自动停止，已采集文件不自动删除。
- 输出限于错误 type/code/message/param、事件名、任务 ID、时间、采集 ID和原始消息 SHA-256。不会保存请求正文、完整 SSE、请求头、认证配置。常见 Bearer/API key/JWT/Cookie/私钥/邮箱/URL 遮盖，message 限 4000 字符。错误消息可能反射业务内容，因此遮盖只能尽力而为，分享前必须人工检查。输出不是完整原始响应体。
- 使用固定输出目录和 UUID 子目录防路径注入，create_new 保证并发限量且不覆盖旧记录。控制文件缺失、损坏、越界或磁盘写入失败不改变代理响应。Unix 文件权限 0600；Windows 使用用户配置目录的既有 ACL。

## 操作入口

诊断版源码构建并经批准安装后，可在不再次重启 CCSM 的情况下运行：

```powershell
./scripts/codex-error-capture.ps1 -Action enable -SessionId 01a05b75-d141-7652-ac6c-a6258653316d -Minutes 10 -MaxEvents 6
./scripts/codex-error-capture.ps1 -Action status
./scripts/codex-error-capture.ps1 -Action disable
```

脚本显示输出目录，通常为用户 `.cc-switch/logs/codex-error-captures/<capture UUID>/`，每条错误单独一个 JSON。控制文件写入成功不等于旧版安装程序支持采集；必须区分源码、安装态和实际新错误文件。本次不安装、不重启、不主动重放真实任务。

## 验证与检索

先运行旧 32 项重试测试全部通过；新增错误终态兼容测试失败，说明原有重连能力与错误显示缺口是两件事。采集测试先失败后实现，覆盖关闭/启用、任务隔离、过期、限量、无效配置、写入失败及凭据遮盖。增加真实用量数据库、失败终态用量、多行 SSE 采集回归。

内置搜索和 Matrix 搜索分别执行；Matrix 搜索无相关结果，不能据此确认技术结论，其 API-reference 读取遇到 403。内置打开官方 Streaming responses 文档确认 error 与 response.completed 是不同事件；具体 Codex 消费行为以本地源码、运行日志、回归测试为依据。没有向检索服务发送任务正文或敏感值。

实现与测试在独立工作树进行，避开同时开发的时区功能；本次不修改该功能。最终测试结果另记于提交说明/交付回复，不把开发中途的部分通过写成安装验收。

最终独立工作树验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib proxy:: -- --quiet --test-threads=4`，1855 passed / 0 failed。新增失败响应元数据保留用例先红后绿，转换保留上游 response.id/model/usage，缺少 ID 时才使用 response.created 的 ID。Cookie、多行 SSE、失败用量用例均有先失败后通过证据。代码只读复审通过；修改文件 rustfmt、git diff --check、UTF-8 严格解码/无 BOM/无 U+FFFD 校验通过。Windows PowerShell 5.1 的 enable/status/disable 在临时目录验证通过，没有启用真实配置目录的采集。

源码已通过 `738a50ea` 合入主工作区，其他任务未提交修改保持原状。合入后在主工作区再次执行同一 proxy 测试命令：1855 passed / 0 failed，包含当时工作区的并发改动；这不是已安装版本的运行验收。主工作区本次文件的 UTF-8/无 BOM 校验通过。
