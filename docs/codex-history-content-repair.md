# 旧对话续聊恢复

旧 CCSM Router 曾把部分智能体的可读回复保存成 `encrypted_content`。代理会在发送
时转换它，切回官方直连后这类记录可能触发 `invalid_encrypted_content`。历史修复的
“旧对话续聊修复”会识别这类记录，将全文恢复为 Codex 原生的 `input_text`。

在 CCSM 的历史记录修复页点击“旧对话续聊修复”，在独立弹窗确认 Codex Home 后
先预览。原有历史修复页面和默认流程保持原样。范围包括
`sessions` 与 `archived_sessions` 下普通 JSONL、Zstd 压缩历史和子任务。此功能与
“侧边栏与索引修复”分开使用，不需要安装界面历史查询补丁。

预览只读取历史，保存不含正文的恢复计划。计划使用摘要绑定；修改目录、待恢复
历史、计划或新增受影响历史后必须重新预览。无关原生历史新增可继续执行，并在
执行时纳入完整备份。预览完成不表示已修复。

实际写入要求先切回官方原生并完全退出 Codex；工具也会保守拒绝正在运行的 ChatGPT
桌面进程。工具不会自动关闭这些进程，也不能
强制绕过进程检查和排他锁。执行前备份全部支持的历史、session index 和有效 SQLite
快照；所有目标核验通过才写入。成功后显示备份目录，并生成执行回执。

恢复保留全文、消息 ID、顺序、Provider、文件路径和每条解压 JSONL 的字节长度，
不改写登录、核心配置、SQLite 及索引。真实密文和不能确定的 Base64 保持原样。
不认识的字段、过期计划、占用或备份失败会拒绝写入；执行失败尝试恢复每个目标。
若恢复失败，保留原始备份并按错误提示处理，不能直接覆盖包含新聊天的数据目录。

## 独立程序

Windows 的 `codex-history-repairer.exe` 无参数时打开窗口，也支持退出 Codex 后在
PowerShell 执行 JSON 命令。其他平台可使用同名 CLI。

```powershell
$tool = (Resolve-Path .\codex-history-repairer.exe).Path
$run = Start-Process -FilePath $tool -ArgumentList '--content-preview' -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput .\preview.json -RedirectStandardError .\preview.error.txt
if ($run.ExitCode -ne 0) { throw (Get-Content -Raw .\preview.error.txt) }
```

无 `--codex-home` 时使用当前生效的 Codex Home。需要指定其他目录时，可在窗口中
选择，或为预览命令传入正确引用的 `--codex-home` 参数。读取 JSON 中的
`previewToken`，确认预览数量和范围，再完全退出 Codex：

```powershell
$preview = Get-Content -Raw -LiteralPath .\preview.json | ConvertFrom-Json
$run = Start-Process -FilePath $tool -ArgumentList @('--content-apply', '--preview-token', $preview.previewToken) -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput .\result.json -RedirectStandardError .\result.error.txt
if ($run.ExitCode -ne 0) { throw (Get-Content -Raw .\result.error.txt) }
```

错误写到 stderr，非零退出码表示操作未成功，详细错误可能包含恢复结果及备份路径。
不要只凭 `result.json` 文件存在判断成功。执行后重新打开 Codex，在原对话发送消息
验收；这属于真实功能验证，不能由“可以读取历史”替代。

零命中是合法结果，表示没有识别出这一类旧格式。此工具不解密真实密文，也不保证
修复其他原因造成的发送失败；保留新错误原文，另行定位。
