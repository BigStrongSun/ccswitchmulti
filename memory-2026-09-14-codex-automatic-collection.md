# Codex 自动采集开发验收（2026-09-14，未安装）

## 实现与边界
- 分支 bigstrongsun/codex-traffic-observability，从530e906b继续。复用已有启动/60秒同步及互斥锁；session_collection统一自动/手动状态，零新增也通知，UTC秒、revision、失败保留最近成功、手动不清除周期deadline。
- 现有状态/流量页展示采集状态；App集中订阅并异步清理，事件只失效缓存，无前端sync定时器。生产子任务查询只读CCSM数据库，旧history/JSONL辅助仅cfg(test)。
- schema24新增checkpoint及结构化子会话metadata。last_seen_at是采集时刻，不是用量发生时刻；范围按usage和有效活动端点确认，跨越窗口不证明窗口内有事件。
- bounded snapshot与追加完整行共享状态机，持久化model/high-water/lane签名/event index。head窗口固定，tail严格止于旧committed offset；修复曾把追加内容哈希进旧tail而导致全部defer的问题。半行/分裂UTF8留到下轮。
- 用量1000条分批事务；先前成功批可保留，final batch与metadata/checkpoint同事务，失败不推进checkpoint，重试稳定request ID去重。不是所有批次单一全文件事务。
- 已知路径及最近日期目录检查，启动和15分钟全发现补偿；canonical containment与visited防外链/环。不新增timer/dependency；常规索引不完整时父依赖defer。
- 非追加重写/截断保留旧账并defer，不自动删除历史消费，需受控Codex重建；重建也清新checkpoint/metadata。局部hash不能绝对识别未命中中段原地修改同时追加。发现补偿不是全文件hash审计，known-path检查仍随文件数增长。
- 普通fork不当subagent。旧父会话可能错含子用量，父直接账保守unknown_may_overlap，不因新metadata就宣称历史归属修复。proxy与session账不相加；cache inclusive input不可再重复加cache。
- 未实现可靠整任务墙钟、质量验收或随机实验；这版不能冒充完整主/子模型净成本与效率排名。

## 最终验证
- 主代理全Rust：4175 passed、0 failed、7 ignored，130.10秒，exit0。日志：C:/Users/sunda/AppData/Local/Temp/ccsm-auto-collection-rust-full-20260914.log。
- focused Rust：Codex/discovery57通过1ignored；usage_stats44、collector3、迁移1通过。全量再次覆盖。
- 主代理前端focused93/93，tsc --noEmit及Vite生产构建exit0；cargo fmt、diff检查与本轮代码UTF8严格解码/no BOM/no U+FFFD通过。
- 全前端首次1593通过/3失败/3unhandled；Terra持久化复跑1595通过/1失败，唯一assertion失败仍为既有AddProviderDialog无receipt mock。App.test当前及530e906b均独立23/23，复跑未复现App异常，不宣称全套绿。主代理读取核对JSON：C:/Users/sunda/AppData/Local/Temp/ccsm-full-suite-20260914.json。
- 浏览器1280x720 fixture验收idle/not_started/degraded与来源文案，明确synthetic；关闭本任务标签页及4178/Vite PID76088，其它服务未动。
- 独立Terra审查未见剩余P1。仍有3个编译warning：checkpoint.file_modified未读、discovery next_full_scan_at未读、测试固定无效UTF8断言，不宣称零warning。

## 提交、检索和上线
- 计划588ef0b1；协调/DB17a9c72b；前端7b5d9309/27a69905/1e5d13e3；读侧收口f855a30c；发现682ca369/272722bf；编译/fixture08edb3a2/5acb5549；parser/schema ee59cc37。均本地提交。
- 内置web与Matrix独立调用；内置无可读返回，双链交叉验证证据不足。Matrix读取Tauri官方calling-frontend文档支持小型事件/异步清理，具体实现由本地源码和真实测试验证。
- 未打安装包、未安装或重启CCSM、未改模型路由、未发起收费实验、未push。开发测试不是安装态证据。下一阶段为受控打包/安装及真实小会话采集验收，保留15721依赖、备份/回滚与监听健康门禁。
