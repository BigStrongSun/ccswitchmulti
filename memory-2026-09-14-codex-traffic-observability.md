# Codex 状态/流量第一期实现与验收

日期：2026-09-14；用户指定Codex多模型路由→状态→流量页，禁止另造统计入口。分支bigstrongsun/codex-traffic-observability，隔离工作树基线216f870e，未动主树protocol_compatibility.rs脏改。

## 根因与实现
- 顶部原称今日总流量但只取最近50条。改为今日最近请求样本，分列非缓存输入、缓存读取/写入、输出及请求平均延迟；不把router诊断事件当请求或0ms。保留原trafficRows给协议/链路证据，新增requestTrafficRows仅用于流量，避免削弱其他状态页。
- 子会话原总数是有界历史列表，与今日范围无关。新增scannedHistoryAgents/inRangeAgents/observedUsageAgents/missingUsageAgents/unknownRangeAgents/historyTruncated；明确不是全库总量，范围内观测看事件时间而非最后updated_at，缺用量不显示零美元。
- 原流量fallback自己解析累计total，会高估父继承与重放；现在通过session_usage_codex::read_verified_codex_rollout_usage复用成熟parser的last优先、签名去重、父prefix剥离及timestamp验证。无法验证则missing，不回填猜测值。
- 该读侧DTO统一cache-normalized非缓存输入；费用计算单独传Codex cache-inclusive raw input，避免再次减cache导致低估。数据底层不迁移、不写真实Codex历史。
- parentGroups只使用session_meta结构化thread_spawn parent_thread_id，冲突关系拒绝，普通fork不算子智能体。父直接用量只取同范围codex_session来源；child依赖rollout回退时父同步可能历史错归属，parentDirectUsage=null并parentUsageStatus=unknown_may_overlap。各层重叠不得相加。
- 请求与会话两路证据不合并、不伪造整体任务耗时或模型效率排名。代理请求轻量自动刷新；会话读取仅进入流量页/手动同步触发，取消周期大规模扫描，完整后台增量采集尚未实现。

## 提交与验证
前端54fe98d9/f4edf3ff/296f1c8a/7bb6f577/cc0eb805；后端86b87012。后续文档提交可由git log追溯。
主agent独立验证：focused前端85/85；TypeScript --noEmit exit0；Vite生产构建exit0；usage_stats41/41；session_usage_codex48/48、1 ignored；cargo fmt --check通过。
全量前端1587通过/1失败（AddProviderDialog.test.tsx新增Codex无receipt场景，缺restore_codex_provider_protocol_evidence mock）。随后原主树同一测试亦7通过/1失败，证明既有基线问题；未扩修无关测试，不宣称全量绿。
浏览器UI：隔离Vite端口3017渲染CodexSessionTrafficPanelFixture，1280×720深色模式检查分项、缺失、两条直接父子分组；模拟数据明确FIXTURE，不是实账或已安装程序证据。临时preview入口不提交。
独立Terra审计最终无剩余P1。已知限制：历史截断、无法验证父/时间戳时missing、父direct同步覆盖不完整、没有整任务墙钟/质量验收/在线随机实验。

## 安装边界
本次没有替换C:/Users/sunda/AppData/Local/CCSwitchMulti/cc-switch.exe，没有重启服务、修改路由、发起模型测试或推送GitHub。开发分支/renderer构建不等于安装态上线，后续安装须独立build/package/受控替换/listener/UI验收。

## 检索
内置web搜索与Matrix独立搜索已使用；内置无可读返回，Matrix精确检索第三方结果不作依据，另open官方Codex app-server文档（重定向learn.chatgpt.com/docs/app-server）确认事件能力背景。具体修复以本地源码和真实测试为证据，不声称完成两链成功交叉验证。
