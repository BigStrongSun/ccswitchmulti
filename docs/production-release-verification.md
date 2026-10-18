# 本地生产构建与安装校验

正式导出将源码提交、前端资源、程序与安装包绑定在同一份清单中。安装事务在
备份和停止程序之前校验清单；新程序启动之前再核对安装态版本与哈希。

## 构建与导出

- `pnpm build`：使用隔离 Cargo target 构建，并检查程序实际内嵌的前端资源。
  开发工作区可以有未提交改动，该入口不生成正式导出的来源凭据。
- `pnpm release:export`：要求 tracked 源码干净，构建后生成 target release
  目录中的 `PRODUCTION-BUILD.json`，验证通过后更新导出目录。
- `pnpm release:local`：使用隔离 Cargo target 和暂存目录，在最终目录替换前
  再次验证导出根目录中的 `PRODUCTION-MANIFEST.json`。
- `-SkipBuild`：必须保留与当前提交、版本、分支、前端及产物一致的构建凭据和
  Tauri 资源缓存。缺少凭据的旧构建需要重新构建；不能按文件时间或版本补签。
  已清理的隔离 target 无法继续复用。

资源检查核对全部 dist 文件与 exe 中实际嵌入的 Brotli 资源。HTML 使用现有
JSDOM 依赖规范化 Tauri 序列化及自动 nonce，其余资源核对字节哈希。

## 安装事务

调用 `scripts/install-ccswitchmulti-transaction.ps1` 时，在原有安装参数之外提供：

- `-ProductionManifestPath`：导出根目录的生产清单路径，保留其相对目录结构。
- `-ExpectedSourceCommit`：预期候选源码的完整 40 位提交 SHA。

安装器路径及调用者传入的预期哈希必须与清单一致。清单中的 raw exe 哈希用于
验证导出文件；NSIS 安装时会替换 Tauri bundle marker，因此安装态哈希由已核验
的同一份 raw 字节重新推导，不能用 raw exe 哈希代替。安装后版本或哈希不匹配
时不会启动候选程序，由已有事务执行回滚。`-PlanOnly` 保留只读预览行为。

`run-v31-local-install.ps1` 是保留的旧机器专用调用器，同样要求上述两项参数；
其既有固定路径和版本不适合作为通用安装模板。

清单用于可信本机构建的来源核对和意外替换检测。构建主机及清单自身仍是信任
边界；这套检查不替代代码签名或对不可信下载的鉴别。

## 自动验证

运行 `pnpm test:release`，使用与现有发布入口相同的 Windows PowerShell 5.1
与 Pester 3.4。CI 的 Windows 作业运行同一入口。测试使用合成资源、临时目录及
安装事务的 Simulation 边界；真实子进程检查只终止测试自身创建的进程。

覆盖缺失构建凭据、旧前端资源、产物替换、错误源码提交、路径越界、错误 NSIS
投影哈希，以及错误安装文件在启动之前被拒绝并回滚等行为。
