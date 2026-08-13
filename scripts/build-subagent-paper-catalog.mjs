import crypto from "node:crypto";
import fs from "node:fs/promises";
import path from "node:path";

const ROOT = process.cwd();
const OUT = path.join(ROOT, "docs/references/subagent-multiagent-2025-2026-papers.json");
const PDF_DIR = path.join(ROOT, "docs/references/papers/subagent-multiagent-2025-2026");
const BIBLIOGRAPHY = path.join(ROOT, "docs/references/subagent-multiagent-2025-2026-annotated-bibliography.md");
const ARCHIVE_README = path.join(PDF_DIR, "README.md");

const studies = [
  ["2026.acl-long.468", "分布式上下文", "当外部知识超过单模型上下文窗口时，多 Agent 是否能分片处理并有效整合？", "ExtAgents 将检索知识分配给并行 Agent，再进行层级整合，并在多跳问答、长综述生成等任务比较非训练型基线。", "增强的多跳问答 ∞Bench+、公开测试集与长综述生成；重点考察输入位于或超过上下文窗口时的知识整合。", "当输入天然可分片且结果可层级汇总时，多 Agent 可以把上下文容量问题转化为分布式知识处理问题。", "结论只适用于论文测试的知识密集任务和编排方式，不能证明任意长任务或任意 Agent 数量都会受益。", ["3.1", "4.2"], [11, 17, 29]],
  ["2026.findings-acl.624", "群体能力", "LLM Agent 群体是否存在可预测跨任务表现的集体智能因子？", "构造 108 个不同规模、模型组成与通信拓扑的 Agent 群体，跨任务测量并提取人工集体智能因子。", "108 个群体，多种规模、模型组合、通信拓扑和任务，用于群体泛化能力预测。", "评价多 Agent 时需要把团队组成和拓扑视为系统变量，不能只看单个模型分数。", "一个统计因子能预测所测任务，不等于存在适用于所有环境的通用 Agent 总分。", ["6.4", "7.2"], [35, 40, 41]],
  ["2026.acl-long.1764", "动态拓扑", "能否按任务动态生成兼顾质量、通信成本与鲁棒性的 Agent 拓扑？", "Guided Topology Diffusion 用离散图扩散逐步生成拓扑，并由轻量代理模型引导多目标奖励。", "多个推理 benchmark，比较静态、手工与生成式拓扑的任务表现、成本和鲁棒性。", "通信拓扑是可优化的系统参数，复杂任务与简单任务可能需要不同稀疏度和结构。", "论文结果不能推出动态拓扑在任意领域都优于人工层级，也不能直接映射为 Codex 的固定 Parent–Child 设计。", ["6.3", "6.6"], [29, 34, 35]],
  ["2026.acl-long.77", "伙伴建模", "显式推断协作者的能力与可信度，能否减少多 Agent 协调失败？", "Explicit Trait Inference 从交互历史推断 warmth 与 competence 两类特征，用结构化伙伴画像指导决策。", "经济博弈与 MultiAgentBench；相对 CoT 基线测量收益损失和任务表现。", "协作需要对伙伴能力形成可更新描述；只有角色名字而没有可观察能力，难以支持稳健选角。", "心理学维度和特定 benchmark 的增益不能外推为所有 Role 描述都会改善协调。", ["7.1", "7.3"], [37, 38, 42]],
  ["2026.acl-demo.34", "系统评价", "为什么多 Agent 评价必须从模型扩展到完整系统？", "MASEval 把框架、拓扑、编排、上下文工程与错误处理作为一等实验变量。", "3 个 benchmark、3 个模型和 3 个框架的系统级比较。", "框架选择可能像模型选择一样显著影响结果，因此 Agent 评价对象应包括 harness 和 orchestration。", "系统演示论文的比较规模有限，不能给所有框架做永久排名。", ["7.4", "7.5"], [40, 41, 42]],
  ["2026.acl-long.1354", "分布式协调反例", "Agent 各自只见局部信息时，能否仅靠自由通信完成分布式计算？", "SILO-BENCH 用无预设角色的精确答案任务，系统改变协议、Agent 数量和模型。", "30 个算法任务、3 个通信复杂度等级、54 种配置、1,620 次实验。", "高通信密度不等于有效协调；任务复杂度和 Agent 数量上升时可能出现 communication–reasoning gap。", "零成功率结论限定于 Level-III、超过 50 Agent 等实验条件，不能说所有小规模层级 Sub-Agent 都会失败。", ["3.4", "6.5", "7.6"], [15, 33, 39, 42]],
  ["2026.acl-long.1294", "澄清与交接", "多 Agent 在消息交接处为什么失败，澄清机制能否阻断错误级联？", "提出 Data Gap、Signal Corruption、Referential Drift、Capability Gap 四类边级错误，并用 AgentAsk 在关键交接处请求最小澄清。", "5 个 benchmark；同时衡量准确率、延迟和额外成本。", "Parent–Child 委托需要清晰的输入、输出和歧义处理机制，通信边本身也是故障面。", "最高 4.69% 增益和低于 10% 开销只属于论文实验，不可写成通用保证。", ["4.3", "7.6"], [18, 19, 42]],
  ["2026.findings-acl.2115", "协同搜索", "多模型独立探索与迭代汇总能否扩大复杂推理的搜索空间？", "MOSA 以 MCTS 为骨架，让多个 LLM 提议、聚合并继续细化推理步骤。", "4 个数学与常识推理 benchmark，比较单 Agent 与其他多 Agent 基线。", "当子问题允许独立探索并可通过统一搜索状态合并时，多 Agent 能提供互补候选路径。", "搜索型 benchmark 的改进不能证明软件工程、工具执行或强共享状态任务同样受益。", ["3.3", "5.3"], [13, 21, 34]],
  ["2026.findings-acl.1694", "辩论反例", "为什么同质 Multi-Agent Debate 可能不如多数投票？", "理论分析均匀信念更新，并引入多样化初始化和显式置信度通信。", "6 个推理型问答 benchmark，对比 vanilla debate、多数投票和两种干预。", "协作收益来自有效差异和可校准信息，而不是重复启动多个同质 Agent。", "经改造 debate 的结果不能外推为所有辩论拓扑优于 Parent–Child，也不能把置信度当真实概率。", ["5.4", "6.4"], [21, 33, 39]],
  ["2026.findings-acl.1712", "专业化流水线", "把长结构化输入拆给专业 Agent 是否比单工作器更连贯？", "内容排序、文本结构和表层实现由三个专业工作器完成，orchestrator 与 guardrail 闭环监督。", "最长 199 个 DBpedia triples，英语和爱尔兰语；人工与 LLM judge 双重评价。", "可验证的专业化流水线可改善长输入生成中的控制与连贯性，但增益可能较小且依赖任务结构。", "爱尔兰语评审相关性不显著且人类与 LLM judge 对齐很弱，不能宣称多 Agent 普遍提升生成质量。", ["4.4", "6.2"], [22, 32, 38]],
  ["2026.findings-acl.1934", "风险评价", "如何在准确率之外评价多 Agent 的协作、工具共享和真实行动风险？", "M-SAEA 用十种探针从模型、工作流、交互和系统四层生成连续风险向量与理由。", "金融管理、网店自动化、交易服务三类高风险任务，覆盖 6 个模型。", "系统验收应包含安全、时效性、工具副作用和交互风险，不能只有任务正确率。", "金融场景探针不能直接证明其他行业的风险阈值，也不能替代人工安全审计。", ["7.5", "7.6"], [41, 42]],
  ["2026.acl-long.1387", "成本压缩", "图结构工作流中哪些 Agent 或模型是冗余的？", "AgentSlimming 估计节点重要性，删除冗余 Agent 或替换低成本模型，并用基线锚定规则防止质量崩塌。", "多个图结构 multi-agent workflow，比较 token 成本与任务质量的 Pareto 关系。", "Agent 数量和模型成本都应按边际贡献验收；可删除的节点说明更多 Agent 并非天然更好。", "最高 78.9% token 降幅属于测试工作流，不能承诺 CCSM 或 Codex 获得同等节省。", ["7.5", "7.6"], [39, 41, 42]],
  ["2026.findings-acl.13", "多样性坍缩", "多 Agent 互动是否真的扩大开放式创意的解空间？", "从模型能力、Agent 认知和系统动力三层分析开放式创意中的语义多样性。", "多模型、多团队权力结构、不同群体规模和通信密度的创意生成实验。", "密集通信和权威耦合可能让 Agent 过早趋同；隔离与独立性有时比更多消息更重要。", "创意多样性结果不能直接等同于代码正确率或事实问答精度。", ["3.5", "6.5"], [15, 33, 39]],
  ["2026.findings-acl.753", "内部失效", "自适应 MAS 的最终正确率是否掩盖内部协调崩坏？", "跨域转移和内部行为分析识别 topological overfitting 与 illusory coordination。", "多种自适应 MAS 在训练域与迁移域上的最终结果和内部协作行为。", "最终答案正确不足以证明协作机制健康，必须检查任务分工、消息与回收过程。", "论文定义的理想 MAS 行为有特定度量，不能未经复核套用到所有运行时。", ["7.4", "7.6"], [40, 42]],
  ["2026.findings-eacl.140", "过程评价", "只评价最终答案会漏掉哪些 Agent 风险？", "立场论文提出过程评价与 compliance score，并用小规模研究展示自动评价可行性。", "医疗、金融、法律、基础设施等敏感应用讨论，加一项小规模过程评价研究。", "Agent 可能通过跳步骤、伪造工具调用或使用过期知识得到表面正确答案，因此要验证过程。", "这是立场论文和小规模验证，不能当作成熟通用评价标准。", ["7.4", "7.6"], [40, 42]],
  ["2026.acl-tutorials.3", "研究综述", "有效且高效的多 Agent LLM 系统由哪些研究问题组成？", "ACL 教程从单 Agent 能力、协作通信与真实应用三个部分梳理方法。", "教程摘要覆盖模型蒸馏、动态路由、记忆/服务效率、图优化、谄媚缓解与应用。", "模型、Agent、通信和服务成本是一个耦合系统，不能只改一个层面后宣布整体最优。", "教程摘要用于建立研究地图，不作为具体性能数字的实验证据。", ["2.1", "5.1"], [4, 24, 43]],
  ["2025.acl-long.421", "协作基准", "如何同时衡量 LLM Agent 的协作与竞争？", "MultiAgentBench 在交互场景中以里程碑 KPI 比较星、链、树、图和认知规划等协议。", "多种协作/竞争场景与多种协调协议；报告任务得分和里程碑达成。", "拓扑改变会改变任务表现，系统评价需要过程里程碑，而非只看最终完成。", "某研究场景中图结构最好、规划提高 3% 不代表所有场景的全局最优。", ["5.2", "6.1", "7.4"], [25, 29, 40]],
  ["2025.emnlp-main.623", "信息传播", "稀疏和稠密拓扑如何传播正确信息与错误？", "构建因果分析框架，并提出融合稀疏/稠密连接的 EIB-Learner。", "多种拓扑稀疏度下比较正确/错误输出传播、通信成本和鲁棒性。", "中等稀疏结构可能在传播有益信息与抑制错误之间取得更好平衡。", "最优稀疏度依赖任务和模型，不能写成固定边数规则。", ["6.1", "6.5"], [29, 34, 35]],
  ["2025.acl-long.1105", "角色差异", "角色如何从标签变成可学习的互补行为？", "Multi-LLM Cooperation 联合学习时序角色嵌入，并用角色差异模块避免收敛。", "7 个数据集上的协作与专业性实验。", "有效角色需要表达任务相关的能力差异；通用的 judge/summarizer 名字未必足够。", "学习式角色嵌入的结果不能证明自然语言 description 一定产生相同互补性。", ["7.1", "7.2"], [37, 38, 39]],
  ["2025.findings-naacl.448", "协调能力", "LLM 在纯协调问题中具备哪些能力、缺少哪些能力？", "LLM-Coordination 包含 4 个纯协调博弈和 198 个 CoordQA 问题，分解环境理解、Theory of Mind 与联合规划。", "Agentic Coordination、CoordQA 和未见伙伴的 zero-shot coordination。", "环境线索驱动的协调相对较强，但需要推断伙伴信念与意图时仍有明显不足。", "纯协调游戏不能完整代表软件工程 Parent–Child 生命周期。", ["5.3", "7.2"], [26, 38, 41]],
  ["2025.emnlp-main.249", "连续协作", "交互环境中 Agent 能否持续理解目标、主动协作并适应伙伴？", "Collab-Overcooked 扩展 Overcooked-AI，并引入过程导向的协作指标。", "13 个 LLM、30 个开放任务和自然语言协作环境。", "模型可能理解目标却仍缺乏主动协作与持续适应；理解任务不等于能形成团队。", "游戏环境的动作和通信约束不能直接外推到代码 Agent。", ["5.3", "7.4"], [26, 40, 42]],
  ["2025.emnlp-main.584", "动态级联", "能否动态选择下一位 Agent 及其可见上下文？", "AnyMAC 使用 Next-Agent Prediction 和 Next-Context Selection 构造任务自适应的顺序协作管线。", "多个 benchmark 上比较静态/图结构基线的表现与通信开销。", "动态选角必须同时决定谁执行和看什么上下文；上下文治理与角色路由不可分割。", "顺序级联的增益不代表并行 Parent–Child 在所有任务上更差。", ["6.3", "7.2"], [35, 38, 56]],
  ["2025.findings-acl.601", "效率训练", "如何联合优化多 Agent 的任务效果、token 效率和通信可读性？", "Optima 采用生成、排序、选择、训练循环，比较 SFT、DPO 与混合方案，并借鉴 MCTS 生成偏好数据。", "信息不对称问答和复杂推理，使用 Llama 3 8B/3.2 3B 等配置。", "多 Agent 的优化目标需要同时包含质量、通信成本和可读性。", "最高 2.8 倍表现与少于 10% token 只属于指定模型、任务和训练设置。", ["7.5", "7.6"], [41, 42]],
  ["2025.acl-long.757", "模型路由", "Multi-Agent 系统怎样联合选择协作模式、角色和模型？", "MasRouter 通过级联控制器依次确定协作模式、角色分配和 LLM 路由。", "代码与推理 benchmark，比较性能、成本和接入不同 MAS 框架的能力。", "多 Agent 模型绑定不是单一模型路由问题，而是协作模式、角色和 provider 的联合决策。", "MBPP/HumanEval 上的数字不能外推为 CCSM 的确定性语义选角或实际节省。", ["7.2", "12.2"], [38, 56, 66]],
  ["2025.emnlp-demos.29", "可配置辩论", "如何系统比较 persona、响应方式、讨论范式和决策协议？", "MALLM 提供 144 种以上可配置组合和统一评估管线。", "Hugging Face 文本数据集、persona、response generator、discussion paradigm 和 voting/consensus。", "Agent 组织不是一个开关；persona、信息流和决策协议需要分别配置和比较。", "框架可配置性本身不证明某个配置有效，也不代表 Codex V2 的官方架构。", ["5.4", "6.4"], [33, 40]],
  ["2025.findings-emnlp.636", "团队初始化", "如何在任务相关性、专业性与多样性之间初始化 Agent 团队？", "AgentInit 通过多轮交互/反思生成 Agent，用格式化机制和 Pareto 团队选择平衡多样性与任务相关性。", "多框架、多任务，比较预定义与自动初始化策略的表现和 token。", "团队组建需要同时考虑能力相关性与多样性；角色描述是系统选择能力的输入。", "最高提升数字依赖论文的初始化方法与任务，不能把 description 当确定性路由规则。", ["7.1", "7.2"], [37, 38, 56]],
];

function decode(text) {
  return text.replaceAll("&amp;", "&").replaceAll("&quot;", '"').replaceAll("&#39;", "'").replace(/<[^>]+>/g, "").replace(/\s+/g, " ").trim();
}

function metas(html, name) {
  const re = new RegExp(`<meta content="([^"]*)" name=${name}>`, "g");
  return [...html.matchAll(re)].map((m) => decode(m[1]));
}

async function downloadPdf(url, localPath) {
  try {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    await fs.writeFile(localPath, bytes);
    return { status: "downloaded", bytes: bytes.length, sha256: crypto.createHash("sha256").update(bytes).digest("hex") };
  } catch (error) {
    return { status: "failed", error: error.message };
  }
}

await fs.mkdir(PDF_DIR, { recursive: true });
const papers = [];
for (const [index, study] of studies.entries()) {
  const [aclId, topic, research_question, method, experiment_scope, supported_conclusion, non_generalizable_boundary, report_sections, deck_slides] = study;
  const paper_url = `https://aclanthology.org/${aclId}/`;
  const pdf_url = `https://aclanthology.org/${aclId}.pdf`;
  const response = await fetch(paper_url);
  if (!response.ok) throw new Error(`${aclId} metadata HTTP ${response.status}`);
  const html = await response.text();
  const title = metas(html, "citation_title")[0];
  const authors = metas(html, "citation_author");
  const venue = metas(html, "citation_conference_title")[0];
  const doi = metas(html, "citation_doi")[0];
  const pages = [metas(html, "citation_firstpage")[0], metas(html, "citation_lastpage")[0]].filter(Boolean).join("–");
  if (!title || !authors.length || !venue || !doi) throw new Error(`${aclId} metadata incomplete`);
  const fileName = `${aclId}.pdf`;
  const localPath = path.join(PDF_DIR, fileName);
  const download = await downloadPdf(pdf_url, localPath);
  papers.push({
    id: `P${String(index + 1).padStart(2, "0")}`,
    acl_id: aclId,
    topic,
    title,
    authors,
    year: Number(aclId.slice(0, 4)),
    venue,
    doi,
    pages,
    publication_status: "formal",
    paper_url,
    pdf_url,
    research_question,
    method,
    experiment_scope,
    supported_conclusion,
    non_generalizable_boundary,
    report_sections,
    deck_slides,
    download: download.status === "downloaded"
      ? { ...download, local_path: path.relative(ROOT, localPath).replaceAll("\\", "/") }
      : download,
  });
  process.stdout.write(`${aclId}: ${download.status}\n`);
}

const catalog = {
  schema_version: "1.0",
  generated_at: "2026-08-13",
  inclusion_rule: "核心计数仅纳入 2025–2026 年 ACL Anthology 正式论文；2026 数量严格多于 2025。",
  search_chains: [
    {
      name: "Codex built-in Web Search",
      independent: true,
      result: "成功召回 ACL Anthology、arXiv 与 OpenAI/Codex 资料；核心元数据回到 ACL 原始页面交叉核对。",
    },
    {
      name: "Matrix WebSearch MCP",
      independent: true,
      result: "已独立检索，但对 2026 ACL multi-agent 查询主要返回日历和无关页面，学术召回不足，未作为论文元数据正证据。",
    },
  ],
  papers,
};
await fs.writeFile(OUT, `${JSON.stringify(catalog, null, 2)}\n`, "utf8");
const bibliography = [
  "# 2025–2026 Sub-Agent 与 Multi-Agent 注释书目",
  "",
  "> 本书目是 `subagent-multiagent-2025-2026-papers.json` 的可读展开。26 篇核心论文全部是 ACL Anthology 正式论文，其中 2026 年 16 篇、2025 年 10 篇。研究结论只在各自实验范围内使用。",
  "",
  "## 检索与纳入方法",
  "",
  "- Codex 内置 Web Search 成功召回 ACL Anthology、arXiv 与相关官方资料；最终元数据逐篇回到 ACL 原始页面核对。",
  "- Matrix WebSearch MCP 使用独立查询，但本轮对 2026 ACL multi-agent 的学术召回不足，主要返回日历等无关结果，因此只记录检索差异，不作为论文正证据。",
  "- 2024 年及更早材料不计入最低数量；预印本可作延伸阅读，但不用于本目录的 26 篇核心计数。",
  "",
  "## 主题地图",
  "",
  "| 问题 | 代表论文 | 阅读结论 |",
  "|---|---|---|",
  "| 为什么拆分 | P01、P08、P10 | 可分片上下文、可合并搜索和专业化流水线能产生收益 |",
  "| 为什么不能盲目加 Agent | P06、P09、P12、P13 | 协调鸿沟、同质辩论、冗余节点和多样性坍缩会抵消收益 |",
  "| 如何组织 | P03、P17、P18、P22 | 拓扑、角色顺序和上下文可见性共同决定信息流 |",
  "| 如何评价 | P02、P05、P11、P14、P15、P21 | 评价对象是系统与过程，不是只有模型和最终准确率 |",
  "| 能力描述为何重要 | P04、P19、P20、P26 | 伙伴能力、角色差异、心智推断和团队多样性影响选角与协作 |",
  "",
  ...papers.flatMap((paper) => [
    `## [${paper.id}] ${paper.title}`,
    "",
    `- **作者：** ${paper.authors.join("；")}`,
    `- **出版：** ${paper.venue}，${paper.year}，${paper.pages}；DOI：[${paper.doi}](https://doi.org/${paper.doi})；状态：正式发表。`,
    `- **研究问题：** ${paper.research_question}`,
    `- **方法与范围：** ${paper.method}${paper.experiment_scope}`,
    `- **本材料可用结论：** ${paper.supported_conclusion}`,
    `- **不可外推：** ${paper.non_generalizable_boundary}`,
    `- **材料映射：** 报告 §${paper.report_sections.join("、§")}；课件第 ${paper.deck_slides.join("、")} 页。`,
    `- **原文与归档：** [论文页](${paper.paper_url}) · [PDF](${paper.pdf_url}) · 本地 \`${paper.download.local_path}\` · SHA-256 \`${paper.download.sha256}\`。`,
    "",
  ]),
].join("\n");
await fs.writeFile(BIBLIOGRAPHY, `${bibliography}\n`, "utf8");

const archiveReadme = [
  "# 论文 PDF 归档清单",
  "",
  "> 用途：教学、研究核对和离线审阅。下载源均为 ACL Anthology 官方 PDF URL；完整注释见上级目录的注释书目。",
  "",
  "| ID | 年份 | 文件 | 字节 | SHA-256 | 来源 |",
  "|---|---:|---|---:|---|---|",
  ...papers.map((paper) => `| ${paper.id} | ${paper.year} | \`${path.basename(paper.download.local_path)}\` | ${paper.download.bytes} | \`${paper.download.sha256}\` | [ACL PDF](${paper.pdf_url}) |`),
  "",
  `归档结果：${papers.length}/${papers.length} 下载成功；失败 0。`,
].join("\n");
await fs.writeFile(ARCHIVE_README, `${archiveReadme}\n`, "utf8");
console.log(`wrote ${papers.length} papers to ${path.relative(ROOT, OUT)}`);
