# Codex 刷新状态截图与安装态核对

用户截图显示配置与运行态正常，分页历史需要处理（8 个历史文件、重复序号 0、8 个轮转任务、20 个续写分段），历史查询兼容层显示重启后验证。截图是状态页，不是一次刷新事务的最终结果。

## 当前只读证据

- 主分支 3a21791a，2026-09-05 00:35:32 提交 canonical renderer V9 修复。源码 codex_desktop.rs 的 MODEL_PICKER_PATCH_KEY 为 __ccSwitchCodexAppCompatibilityV9。75c209c2 在 2026-09-04 22:29:53 修正活动 history_base lineage 与兄弟分支误合并。
- 实际进程 cc-switch.exe PID 65480，启动时间 2026-09-05 01:03:13，路径 C:/Users/sunda/AppData/Local/CCSwitchMulti/cc-switch.exe。安装文件修改时间 2026-09-04 19:12:48，内嵌版本 3.19.2-29，SHA256 D73F12AA6F2C5E83D876237A53879E99761BBE57BA7D0FF607D74322B5EBD832。二进制只有兼容脚本 V7 标识，没有 V8/V9、活动 lineage 新错误标识和 proxy-errors.jsonl 标识。
- 本地 src-tauri/target/release/cc-switch.exe 修改时间 2026-09-05 00:37:30，同样内嵌版本 3.19.2-29，但 SHA256 221B8DFD8238923AD963C3C707F158AE3E91F4B5E30535BE2A13F3C4C7818BD5。二进制为 V8，包含活动 lineage guard 和新错误日志，但没有 V9 或 getCompleteConversationTurns。不能仅凭文件时间晚于提交或相同版本号认为包含最新修复。
- Desktop ChatGPT.exe PID 57456 的父进程为 CCSM 65480；当前路径属于 OpenAI.Codex_26.901.4073.0。app-server PID 31492 的父进程为 57456。当前桌面版本不同于此前 V9 模拟/源码调查所对照的 26.901.2854，最新修复仍需安装后验证。
- cc-switch.log 第 42946 行记录 01:03:36 确实合并过任务 01a02011-571b-70d3-af29-c412af8bed36 的两个分段；对应备份目录存在，包含 54253 与 4001047 字节的原始文件。这只能证明执行过部分磁盘修复，不能证明全部历史或 renderer 已修复。
- CodexConfigConsistencyDialog.tsx 状态页的兼容层标签直接使用 statusPending（重启后验证），并不根据一次即时 renderer 检查结果计算。分页告警来自 preflight 的 affectedRolloutCount/blockedRolloutCount；重复序号为零也可能因轮转分段触发，不能解释成“完全没有问题”。

## 结论与下一步

主要归类为“最新修复已在主分支，但未实装”；同时状态页存在固定待验证标签，不能用它判断真实成功/失败。现有 release EXE 也没有最新 V9，不应直接把它当作最新修复安装。需要从明确包含 3a21791a 的源码重新构建，校验 V9 和产物哈希，再经授权通过可回滚升级事务替换运行安装，最后独立验收配置、磁盘历史、renderer 历史。未经实际验收不能声称 V9 已解决当前新桌面版本的问题。

本次只读取证，没有重启、关闭、替换 CCSM/Codex，没有点击破坏性刷新或修改真实 rollout。内置搜索与 Matrix 搜索分别执行；官方故障排查页仅提供通用诊断，Matrix 结果不相关，两者都不证明本机修复状态。结论由进程路径、二进制标识/哈希、源码和本机修复日志交叉确认。

## 首次实装失败与新增根因

- 2026-09-05 01:35 从提交 16bf866b 重建完成，冻结产物确认包含 V9、`getCompleteConversationTurns`、活动 lineage 诊断和 `proxy-errors.jsonl`；原始 EXE SHA256 为 `5DCFD77B9BF1339CF931F4D32E9DB89975BEB5CB30EECBBFCB8A0579BC0BA71A`，安装后预期 SHA256 为 `80BE1D15AB7CEC36DA0F89EC23BD2B38C2795A2E5CADB0AB3F2A5FD5646497ED`。
- 可回滚事务停止 PID 65480 后，端口 15721 仍以已经不存在的 65480 为 owner 保留到 120 秒超时。备份事务 `ccsm-20260905-013527-afff442ed28347649a4ced16634d212c` 记录 `timed out waiting for port 15721 release`。旧安装目录与 SHA256 已恢复，guardian 后续恢复健康服务。
- 运行时链路证明 Codex Desktop 是 CCSM 的子进程。CCSM 启动 Codex 时没有显式隔离标准句柄；Windows 子进程保留了代理监听句柄，导致父进程退出后端口仍不释放。修复应在启动边界把 Codex 的 stdin/stdout/stderr 显式接到 NUL，避免继承 CCSM 的运行时句柄，不能把事务等待时间继续加长来掩盖问题。
- 当前旧安装首次升级仍需要一次 Codex Desktop 退出才能释放已经继承的旧句柄。后续由修复版 CCSM 启动的 Codex 不应再阻塞 CCSM 独立升级。未经一次真实的“Codex 保持运行、CCSM 单独重启、端口正常释放”验收前，不宣称该根因已在安装态验证。
