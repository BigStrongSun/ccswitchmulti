import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const deckDir = path.join(root, "docs/presentations/codex-v2-subagent-report");
const catalog = JSON.parse(fs.readFileSync(path.join(root, "docs/references/subagent-multiagent-2025-2026-papers.json"), "utf8"));

const C = {
  intro: "chapter-intro",
  summary: "chapter-summary",
};
const s = (id, chapter, title, plain, body, papers = [], cls = "") => ({ id, chapter, title, plain, body, papers, cls });
const p = (...items) => items.map((x) => `<p>${x}</p>`).join("");
const cards = (...items) => `<div class="cards">${items.map(([h, t]) => `<article class="card"><h3>${h}</h3><p>${t}</p></article>`).join("")}</div>`;
const cite = (ids) => ids.map((id) => `<button class="paper-cite" data-paper="${id}">[${id}]</button>`).join(" ");
const source = (text, ids = []) => `<p class="source">证据 · ${text} ${cite(ids)}</p>`;
const summary = (yes, no) => `<div class="boundary"><div><b>能得出的结论</b><p>${yes}</p></div><div><b>不能外推的结论</b><p>${no}</p></div></div>`;
const flow = (...nodes) => `<div class="flow">${nodes.map((n) => `<span>${n}</span>`).join("")}</div>`;
const table = (heads, rows) => `<div class="table-card"><table><thead><tr>${heads.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((x) => `<td>${x}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`;

const slides = [
s("cover", "导论", "Codex V2 Sub-Agent：从多智能体研究到 CCSM 第三方模型适配", "这是一份从零开始的学习材料：先弄懂 Agent 为什么要分工，再看 Codex 的 V1、V2，最后看 CCSM 怎样让第三方模型真正成为 Child。", p("本材料把 2025–2026 年多智能体研究、Codex 官方实现与 CCSM 源码证据放进同一条因果链。它不是运维手册，也不把“多开几个模型”包装成智能跃迁。", "核心问题是：什么任务值得委托、组织结构怎样改变信息流、V1 为什么走向 V2，以及跨 Provider 时究竟坏在哪一层。") + `<div class="hero-stat"><b>68</b><span>阅读页</span><b>26</b><span>核心正式论文</span><b>3</b><span>可切换主题</span></div>` + source("ACL Anthology 2025–2026；OpenAI 文档/源码；CCSM 源码/测试/运行证据"), [], "cover"),
s("questions", "导论", "五个研究问题决定这份材料的阅读顺序", "我们不是先背 V1、V2 名词，而是先回答五个问题：为什么拆、怎么组织、怎么评价、Codex 为什么变、CCSM 到底改了什么。", cards(["为什么需要 Sub-Agent？", "单 Agent 的上下文、串行和自验证边界何时超过拆分成本？"],["为什么叫 Sub-Agent？", "Sub 描述哪一种授权、目标、责任和生命周期关系？"],["与 Multi-Agent 有何不同？", "层级式 Parent–Child 在更大的多智能体集合中处于什么位置？"],["V1 为什么走向 V2？", "从运行实例到协作任务树，解决了哪些组织问题？"],["CCSM 如何适配第三方？", "控制面、Provider 物化与消息投影分别承担什么？"])),
s("evidence-method", "导论", "证据不是堆链接：每一类来源回答不同问题", "论文用来回答一般机制，官方源码回答 Codex 现在怎么跑，CCSM 测试回答兼容层做了什么，真实运行才证明某个环境里真的成功。", table(["证据层", "能证明", "不能代替"], [["PAPER", "指定实验中的机制、收益与反例", "Codex 当前字段和 CCSM 运行"],["O-DOC / O-SRC", "官方语义、数据结构、调用顺序", "受影响机器成功"],["C-SRC / C-TEST", "实现边界与回归契约", "真实 Provider 已命中"],["RUNTIME", "某版本、某环境的真实结果", "所有平台永久有效"]]) + source("论文目录是唯一结构化来源；数字只在原实验条件内使用", ["P05","P15"])),
s("reading-map", "导论", "从研究到 Codex/CCSM：四层收敛路线", "先学通用概念，再研究组织和评价，然后看 Codex V1/V2，最后才进入第三方模型和 CCSM。这样不会把产品实现误当成学术定义。", flow("Agent 基础", "拆分条件", "Sub-Agent / MAS", "组织与评价", "Codex V1→V2", "第三方障碍", "CCSM 完整链路") + summary("读者能建立从任务结构到运行实现的因果链。", "不能把 Codex 的 V1/V2 工程分期当成整个学界的统一标准。")),

s("agent-chapter", "Agent 基础", "第二章导览：先分清 Model、Agent 与运行系统", "这一章解决最基础的混淆：模型负责生成，Agent 负责循环，运行系统负责工具、权限、状态和停止。", cards(["本章问题", "Agent 比一次模型调用多了哪些结构？"],["关键概念", "Model、Agent、harness、tool call、state、side effect。"],["证据入口", "系统评价研究说明框架选择也会显著改变结果。"])+source("MASEval；ACL 多智能体教程",["P05","P16"]),["P05","P16"],C.intro),
s("model-agent-system", "Agent 基础", "Model、Agent、Harness 是三个不同层次", "模型像大脑的一次思考；Agent 是会反复观察、决定、行动的工作者；Harness 是给它工具、权限、记忆和停止规则的工作环境。", table(["层次","核心职责","典型证据"],[["Model","生成、推理、选择候选动作","模型输出"],["Agent","围绕目标运行观察—决策—行动循环","完整轨迹"],["Harness/runtime","装配上下文、工具、权限、重试与生命周期","源码、配置、日志"]])+source("框架选择可能与模型选择同样重要，限定于 P05 的比较范围",["P05"]),["P05"]),
s("agent-loop", "Agent 基础", "单 Agent 的最小闭环：观察、决策、行动、再观察", "Agent 不是回答一次就结束。它读取现状，决定下一步，调用工具看到结果，再判断继续还是停止。", `<div class="loop"><span>目标 / 状态</span><span>模型决策</span><span>工具行动</span><span>环境反馈</span><span>验证 / 终止</span></div>`+p("闭环使 Agent 能处理真实副作用，也引入了新风险：工具可能失败、状态可能过期、模型可能误以为已经执行。过程评价因此不可缺少。")+source("Process Evaluation",["P15"]),["P15"]),
s("harness-tools-state", "Agent 基础", "Harness 把模型建议变成受约束的真实行动", "真正让 Agent 能干活的，是运行系统把工具、状态、权限和重试接起来；换一个 Harness，同一个模型也可能表现完全不同。", cards(["工具", "把读文件、搜索、执行、写入变成有 schema 的动作。"],["状态", "保存目标、消息、工具结果和可恢复进度。"],["权限", "决定可读写范围、沙箱、审批与敏感边界。"],["生命周期", "启动、等待、恢复、失败、取消和终止。"])+source("系统级评价把 harness/context engineering 设为一等变量",["P05"]),["P05"]),
s("single-agent-boundary", "Agent 基础", "单 Agent 的能力边界不是“模型不够聪明”这么简单", "一个 Agent 同时读论文、查代码、跑测试、做审查时，真正紧张的是上下文、时间和自我验证，而不只是模型参数。", cards(["上下文竞争", "中间日志挤压目标、约束和关键证据。"],["串行关键路径", "独立证据面被迫一个接一个处理。"],["能力冲突", "探索、实现、审查需要不同工具和权限。"],["自我确认", "同一轨迹更容易重复自己的假设。"])),
s("single-agent-case", "Agent 基础", "一个单 Agent 软件任务：直接，但所有责任堆在同一条轨迹", "比如定位跨平台开机启动回归：单 Agent 要依次查 Windows、macOS、Linux，再追 Git 和测试。任务能完成，但主线程会混入三套平台细节。", flow("读三平台代码", "查提交历史", "复现与测试", "比较共同根因", "写结论")+p("如果平台调查互相独立，前三步可以委托；如果修复共同模块且共享状态强耦合，则仍应由一个拥有写入权的主线整合。")),
s("agent-summary", "Agent 基础", "本章结论：问题不只在模型，系统才是评价单位", "同一个模型放进不同工具、上下文和权限系统，表现会不一样；所以不能拿模型榜单直接当 Agent 等级。", summary("Agent 是模型与运行系统共同形成的闭环；任务、工具、状态、权限和终止都影响表现。", "不能从模型基准分数推断某个 Agent 工作流必然可靠。")+source("MASEval；风险优先系统评价",["P05","P11"]),["P05","P11"],C.summary),

s("why-split-chapter", "单 Agent 的瓶颈", "第三章导览：什么条件让拆分开始有价值", "这一章不急着赞美多智能体，而是先逐项计算：上下文、目标漂移、并行、能力冲突和验证收益是否真的超过协作成本。", cards(["收益候选", "上下文分片、并行、能力互补、独立审查。"],["新增成本", "通信、汇总、等待、写冲突、错误传播。"],["判断标准", "子目标可委托、可隔离、可验收。"])+source("ExtAgents；SILO-BENCH",["P01","P06"]),["P01","P06"],C.intro),
s("context-competition", "单 Agent 的瓶颈", "上下文竞争：信息越多，不等于决策越完整", "主线程装入所有搜索结果时，重要约束可能被过程噪声淹没。把知识分片给 Child 有价值，但前提是结果能压缩、能汇总。", `<div class="compare-bars"><div><b>单线程</b><i style="--w:92%">目标 + 日志 + 论文 + 测试</i></div><div><b>Parent + Child</b><i style="--w:48%">目标 + 可验证摘要</i></div></div>`+p("ExtAgents 在知识密集任务中支持分布式输入整合，但这不是对任意长任务的普遍保证。")+source("ExtAgents 的 ∞Bench+、公开测试集和长综述实验",["P01"]),["P01"]),
s("goal-drift", "单 Agent 的瓶颈", "长任务目标漂移：执行细节会逐渐替代最初目标", "Agent 在长轨迹中可能把“完成某个局部步骤”误当成“完成用户目标”。拆分可以隔离局部过程，但 Parent 必须保留验收责任。", cards(["局部成功", "Child 完成一次搜索或代码修改。"],["全局成功", "用户要求的跨平台根因、修复和验证全部闭环。"],["治理要求", "Parent 对完成标准、冲突与最终承诺负责。"])+source("内部协调可能在最终结果表面成功时已经偏离",["P14"]),["P14"]),
s("parallel-subproblems", "单 Agent 的瓶颈", "并行只适合真正独立的子问题", "Windows、macOS、Linux 的只读调查可以并行；同一个数据库迁移文件的三路修改不能简单并行。", table(["可并行","不宜并行"],[["不同平台只读证据","同一文件并发写入"],["互不依赖的论文主题","每一步依赖前一步状态"],["独立反例审查","没有统一验收标准的开放任务"]])+source("MOSA 展示独立搜索路径和统一合并状态的价值",["P08"]),["P08"]),
s("capability-conflict", "单 Agent 的瓶颈", "能力与工具冲突：探索者、实现者、审查者不是同一岗位", "角色分工的价值不在于换名字，而在于工具、权限、模型能力和交付标准真的不同。", cards(["Explorer", "只读、快速、长上下文证据收集。"],["Worker", "拥有明确文件所有权和实现责任。"],["Reviewer", "独立寻找反例、权限风险和验证缺口。"])+source("角色差异需要任务相关行为；伙伴能力可被显式建模",["P04","P19"]),["P04","P19"]),
s("self-verification", "单 Agent 的瓶颈", "自我验证的盲点：同一轨迹容易重复同一个假设", "独立审查可以降低确认偏差，但如果所有 Agent 同模型、同提示、同上下文，所谓“独立意见”可能只是重复采样。", p("Vanilla multi-agent debate 在同质 Agent 和均匀信念更新下，可能不如多数投票却消耗更多计算。真正有价值的是观点差异、可校准置信度和独立证据。")+source("Demystifying Multi-Agent Debate",["P09"]),["P09"]),
s("split-summary", "单 Agent 的瓶颈", "本章结论：先证明值得拆，再决定拆给谁", "最重要的不是能不能启动 Child，而是子目标是否独立、边界是否清楚、结果是否能验收，收益是否大于沟通和整合成本。", summary("可分片上下文、独立证据面和真实能力互补可以支持拆分。", "任务长或模型贵本身不是充分理由；更多 Agent 也不是能力证明。")+source("正向与反例证据并列",["P01","P06","P09","P12"]),["P01","P06","P09","P12"],C.summary),

s("subagent-chapter", "Sub-Agent", "第四章导览：Sub-Agent 是一种层级委托关系", "这一章正式回答为什么名字里有 Sub：它描述 Child 对 Parent 的目标、授权、责任和生命周期从属，而不是模型更小。", cards(["定义", "Parent 在同一用户目标下委托有边界子任务。"],["边界", "Child 有独立上下文/生命周期并返回可整合结果。"],["反例", "工具调用、并行采样和独立聊天不自动成为 Sub-Agent。"]),[],C.intro),
s("subagent-definition", "Sub-Agent", "Sub-Agent 的正式定义：有目标、有边界、有回收", "Sub-Agent 是 Parent 为同一总目标创建的 Child：它接收一项清晰委托，在隔离上下文中工作，并把证据和结果交回 Parent。", `<blockquote>Sub-Agent = Child Agent + delegated goal + bounded scope + independent lifecycle + return contract</blockquote>`+p("这个定义关注治理结构，不限制模型厂商、模型大小或价格。")),
s("parent-child", "Sub-Agent", "Parent 与 Child 的责任不对称", "Parent 负责全局目标、选角、冲突和最终验收；Child 负责局部任务、局部证据和明确边界。两者不是两个平级总负责人。", table(["责任","Parent","Child"],[["目标","保留用户总目标","接收局部目标"],["范围","划定权限与写入所有权","不越界扩张"],["验收","比较、验证、整合","提交产物、证据与不确定性"],["生命周期","等待、追问、终止","完成或报告阻塞"]])),
s("delegation-contract", "Sub-Agent", "委托契约至少包含六项，不是一句“帮我看看”", "任务交接如果缺目标、输入或输出格式，错误会在 Agent 之间放大。一个好委托像接口契约，可以被检查和追问。", cards(["目标", "要回答的具体问题。"],["范围", "文件、系统、写权限和禁止项。"],["输入", "必要上下文与已知证据。"],["输出", "结论、产物、格式和引用。"],["验收", "什么证据算完成。"],["终止", "完成、失败、超时和取消条件。"])+source("AgentAsk 的四类边级错误与澄清机制",["P07"]),["P07"]),
s("tool-vs-subagent", "Sub-Agent", "Sub-Agent 与 Tool Call：是否拥有独立目标和生命周期", "工具像扳手：执行一个动作后返回。Sub-Agent 像被委托的同事：理解局部目标、自己选择多个动作、维护状态并交付结果。", table(["维度","Tool Call","Sub-Agent"],[["目标","调用者已决定的单步动作","可自行规划的局部目标"],["上下文","参数与调用现场","独立/隔离的 Agent context"],["生命周期","一次调用","可等待、追问、打断、恢复"],["结果","函数返回值","可整合产物、证据与边界"]])),
s("ensemble-vs-subagent", "Sub-Agent", "Sub-Agent 与并行采样：多个答案不等于协作", "同一提示生成五份答案再投票，是 ensemble；五个角色分别调查、通信、验证和回收，才形成多智能体协作。", p("并行采样没有稳定责任分工，也不必共享任务状态。把它叫 Sub-Agent 会掩盖真正的治理成本。")+source("同质 debate 可能不如多数投票",["P09"]),["P09"]),
s("parent-child-case", "Sub-Agent", "案例：跨平台回归调查如何形成 Parent–Child 结构", "Parent 把 Windows、macOS、Linux 的只读证据分别委托，自己追踪共同提交并判断是共因还是平台特异问题。", flow("Parent 定义验收", "Win Child", "Mac Child", "Linux Child", "Parent 交叉比较", "单一修复主线")+p("Child 不同时改公共启动模块；写入所有权仍集中，避免把上下文竞争换成合并冲突。")),
s("subagent-summary", "Sub-Agent", "本章结论：Sub 表示治理关系，不表示低配模型", "只要存在 Parent 的委托、边界和回收，Child 就是 Sub-Agent；它可以用更强模型，也可以与 Parent 来自不同 Provider。", summary("Sub-Agent 是层级式委托；核心结构是目标、授权、责任、生命周期与回收。", "不能由模型便宜、进程独立、窗口数量或并行次数判断它是 Sub-Agent。"),[],C.summary),

s("mas-chapter", "Sub-Agent 与 Multi-Agent", "第五章导览：Sub-Agent 是 Multi-Agent 的层级式子集", "Multi-Agent 是大集合，包含平级协作、竞争、辩论、流水线和层级结构；Sub-Agent 专指其中有 Parent–Child 委托的结构。", cards(["广义 MAS", "两个或更多 Agent 通过协议共同或竞争完成任务。"],["层级子集", "Parent 持有总目标，Child 接受局部委托。"],["判断问题", "谁有最终决策权？结果回到哪里？"]),[],C.intro),
s("mas-definition", "Sub-Agent 与 Multi-Agent", "Multi-Agent 的广义定义：成员关系可以协作，也可以竞争", "多智能体系统关注多个 Agent 之间的组织、通信和决策。成员可以共享目标，也可以通过竞争、投票或博弈形成结果。", cards(["协作", "共享目标、交换证据、共同产出。"],["竞争", "独立提出方案，由规则选择。"],["混合", "局部竞争探索，最终层级汇总。"])+source("MultiAgentBench 同时评价协作与竞争",["P17"]),["P17"]),
s("subset-relation", "Sub-Agent 与 Multi-Agent", "包含关系：每个 Sub-Agent 系统都是 MAS，但不是每个 MAS 都有 Sub-Agent", "平级 debate 没有 Parent–Child 从属，所以是 MAS 但不是这里定义的 Sub-Agent；一个 Parent 创建多个 Child，则同时属于两者。", `<div class="venn"><div>Multi-Agent System<div>Hierarchical MAS<div>Sub-Agent</div></div></div></div>`),
s("cooperate-compete", "Sub-Agent 与 Multi-Agent", "协作、竞争和混合：不同关系需要不同回收规则", "协作需要共享足够信息，竞争需要保持独立性，混合结构要明确何时停止探索、由谁裁决。", table(["关系","信息策略","决策方式"],[["协作","共享必要状态","共同产物或合并"],["竞争","隔离初始路径","投票、judge、规则选择"],["混合","先隔离后汇总","Parent 分阶段裁决"]])+source("多样性与置信度决定 debate 是否有价值",["P09","P13"]),["P09","P13"]),
s("static-dynamic", "Sub-Agent 与 Multi-Agent", "静态团队与动态团队：角色、上下文和连接可以在运行中变化", "静态团队预先定义谁做什么；动态团队会根据当前任务决定下一位 Agent、可见历史和通信拓扑。", p("AnyMAC 同时做 Next-Agent Prediction 和 Next-Context Selection；MasRouter 联合选择协作模式、角色和模型。这些研究说明，路由不是只看模型名。")+source("动态级联与 MAS routing",["P22","P24"]),["P22","P24"]),
s("mas-summary", "Sub-Agent 与 Multi-Agent", "本章对照：判断关系要看权责和信息流", "如果最终目标、授权和结果回收集中在 Parent，就是层级式 Sub-Agent；如果成员平级协商或竞争，则是更广义的 MAS。", summary("Sub-Agent 是层级 MAS 子集，适合可委托、可回收的任务树。", "不能把所有多模型调用都叫 Multi-Agent，也不能把所有 MAS 都画成 Parent–Child。"),[],C.summary),

s("topology-chapter", "组织结构", "第六章导览：组织结构决定谁看见什么、谁作决定", "拓扑不是画图风格，它决定信息传播、错误放大、token、延迟和回收点。", cards(["比较维度", "信息流、决策权、关键路径、错误传播、成本。"],["基本结构", "星、层级、链、debate、树、一般图。"],["核心结论", "没有脱离任务的全局最优拓扑。"])+source("MultiAgentBench；信息传播；动态拓扑",["P03","P17","P18"]),["P03","P17","P18"],C.intro),
s("topology-information", "组织结构", "组织结构首先是信息流设计", "同一批 Agent 换一种连接方式，正确消息和错误消息到达的位置、速度和次数都会改变。", `<div class="topologies"><span>★ Star</span><span>⇢ Chain</span><span>♜ Tree</span><span>◎ Debate</span><span>⬡ Graph</span></div>`+source("拓扑改变任务里程碑与错误传播",["P17","P18"]),["P17","P18"]),
s("star", "组织结构", "Parent–Child 星形：清晰回收，但 Parent 可能成为瓶颈", "所有 Child 向 Parent 返回，容易控制权限和验收；Child 之间默认隔离，能减少写冲突和错误扩散。", cards(["优势", "责任清楚、回收点单一、容易审计。"],["代价", "Parent 汇总负担大，跨 Child 信息要转发。"],["适用", "独立调查、局部实现、独立审查。"])),
s("hierarchy", "组织结构", "多层层级：扩大分解规模，也放大摘要损失", "中间 Parent 可以管理局部团队，但每增加一层，就增加一次目标翻译、摘要和故障归因。", flow("Root", "Domain Parent", "Specialist Child", "Local verifier", "逐层回收")+p("层级适合能形成稳定子树的大任务，不适合频繁跨层共享细粒度状态。")),
s("pipeline", "组织结构", "链式流水线：阶段清楚，但早期错误会一路传下去", "排序、结构、表面生成是典型流水线。每个阶段有清楚输入输出，但上一步漏掉信息时，下游可能无法恢复。", flow("Content ordering", "Text structuring", "Surface realization", "Guardrail")+source("三 worker 长 triple-set 生成只获得有限、语言相关的改善",["P10"]),["P10"]),
s("debate", "组织结构", "Debate：需要真实差异和置信度，而不是同质复读", "平级 Agent 相互批评看起来很热闹，但如果初始观点相似、更新规则相同，讨论不会凭空产生新证据。", cards(["需要", "独立初始路径、观点多样性、可校准置信度。"],["风险", "谄媚、趋同、token 膨胀、无休止讨论。"])+source("vanilla debate 与改进机制",["P09","P25"]),["P09","P25"]),
s("tree-graph", "组织结构", "树形探索与一般图：搜索空间更大，终止和通信更难", "树适合保留多条候选路径，图允许任意依赖和反馈；结构越自由，越需要明确通信预算和停止规则。", p("MOSA 用 MCTS 统一多 Agent 搜索状态；MultiAgentBench 显示不同拓扑在不同场景表现不同。")+source("协同搜索与拓扑比较",["P08","P17"]),["P08","P17"]),
s("dynamic-topology", "组织结构", "动态选角与动态拓扑：把结构本身纳入优化", "简单任务不需要密集团队，复杂任务也不一定适合固定结构。动态方法按质量、成本和鲁棒性共同选择连接。", p("Guided Topology Diffusion 生成任务自适应稀疏拓扑；AnyMAC 动态决定下一角色和上下文。")+source("动态拓扑与级联",["P03","P22"]),["P03","P22"]),
s("topology-summary", "组织结构", "本章结论：不存在脱离任务、模型和预算的最优拓扑", "星形适合清晰委托，链式适合稳定阶段，debate 需要差异，图适合复杂依赖；每种结构都有独特失败方式。", summary("拓扑应围绕信息流、错误传播和成本选择，并随任务复杂度调整。", "不能把某个 benchmark 中的图结构优势写成一般规律，也没有固定“最佳 Agent 数”。")+source("稀疏度、动态拓扑与协调鸿沟",["P03","P06","P18"]),["P03","P06","P18"],C.summary),

s("evaluation-chapter", "角色与评价", "第七章导览：角色必须描述能力，评价必须观察系统", "角色名只是标签。真正影响协作的是能力、工具、上下文、权限和验证方式；评价也不能只看最终准确率。", cards(["角色问题", "为什么叫 expert 不等于真的适合？"],["评价问题", "怎样区分模型能力与系统能力？"],["失败问题", "答案对了，内部协作是否仍可能坏？"])+source("角色差异；MASEval；内部失效",["P05","P14","P19"]),["P05","P14","P19"],C.intro),
s("role-not-name", "角色与评价", "角色不能只是名字：它需要可执行的能力描述", "“专家”“Worker”这样的名字没有告诉调度者它擅长什么、不该做什么、有哪些工具、能否写入。", table(["弱角色","强能力描述"],[["expert","擅长复杂调试；可改代码；需返回测试与风险"],["researcher","只读长文献；提取实验范围；不做实现"],["reviewer","独立反例审查；不接管最终整合"]])+source("任务相关角色差异",["P19"]),["P19"]),
s("capability-description", "角色与评价", "能力描述是语义选角信号，不是硬路由规则", "描述帮助 Parent 把任务与角色的擅长范围对齐，但模型仍在做语义判断，模糊任务可能选错。", p("Explicit Trait Inference、AgentInit 和 MasRouter 分别说明伙伴能力、团队组成与联合路由的重要性；它们不证明自然语言 description 能确定性命中。")+source("伙伴能力、初始化与路由",["P04","P24","P26"]),["P04","P24","P26"]),
s("homogeneous", "角色与评价", "同质 Agent 为什么可能没有收益", "多个 Agent 如果共享相同模型、提示、证据和更新规则，往往共享同一盲点；更多对话只会放大成本或加快趋同。", cards(["同质 debate", "可能不如多数投票。"],["密集通信", "可能加速多样性坍缩。"],["冗余节点", "删除后质量不降、成本反而下降。"])+source("三类反例",["P09","P12","P13"]),["P09","P12","P13"]),
s("model-to-system", "角色与评价", "从模型评价转向系统评价", "Agent 系统表现由模型、Harness、拓扑、编排、上下文和错误处理共同决定。只换模型、不检查系统，无法解释问题来自哪里。", `<div class="equation">System outcome = Model × Harness × Topology × Orchestration × Environment</div>`+source("MASEval 的系统级比较；人工集体智能因子",["P02","P05"]),["P02","P05"]),
s("capability-vector", "角色与评价", "Agent 能力向量：九个维度比一个总分更诚实", "一个 Agent 可能会写代码却不会安全终止，也可能会搜索但不会验证。用向量能看见具体短板，不会被总分掩盖。", `<div class="vector">${["任务分解","角色匹配","工具使用","上下文/状态","协作通信","结果验证","终止恢复","成本鲁棒性","安全权限"].map((x,i)=>`<span><b>${i+1}</b>${x}</span>`).join("")}</div>`+source("过程、风险与协作评价",["P11","P15","P21"]),["P11","P15","P21"]),
s("cost-risk", "角色与评价", "成本、延迟、错误传播和安全都属于能力", "多 Agent 可能并行降低墙钟时间，也可能因为汇总和通信增加 token；一条错误消息还能沿拓扑扩散。", table(["维度","至少记录"],[["质量","任务结果、里程碑、过程合规"],["成本","token、模型价格、通信密度"],["延迟","关键路径、等待、重试"],["鲁棒性","局部失败、恢复、拓扑迁移"],["安全","权限、工具副作用、敏感数据"]])+source("成本压缩、风险优先和信息传播",["P11","P12","P18","P23"]),["P11","P12","P18","P23"]),
s("evaluation-summary", "角色与评价", "本章结论：多 Agent 必须验收协作过程", "最终答案只是结果的一部分。需要知道谁做了什么、消息是否完整、工具是否真的执行、错误如何恢复、成本是否值得。", summary("评价应覆盖模型、harness、拓扑、编排、过程、成本和安全。", "不能用一个总分永久评级 Agent，也不能用最终正确率证明内部协调健康。")+source("内部失效与过程评价",["P14","P15"]),["P14","P15"],C.summary),

s("v1-chapter", "Codex V1", "第八章导览：V1 是以运行实例为中心的 Sub-Agent", "V1 已经能启动、追问、等待和关闭 Child。它的核心身份是 Agent ID，协作任务含义主要由 Parent 自己记。", cards(["它是什么", "围绕 agent id 管理 Child 生命周期。"],["它能做什么", "spawn、send/resume、wait、close。"],["它的边界", "任务层级与协作语义主要留在 Parent 上下文。"]),[],`${C.intro} v1-overview`),
s("v1-id", "Codex V1", "V1 用 Agent ID 管理运行实例", "Parent 创建 Child 后拿到一个 ID，后续操作都围绕这个句柄。机制直接，但任务多时 Parent 要自己维护 ID 对应的任务。", flow("spawn_agent", "agent_id=a1", "send/resume", "wait", "close")+source("Codex V1 handler 与 AgentControl 源码审计")),
s("v1-structure", "Codex V1", "V1 Parent/Child 结构：实例集合，而不是规范任务树", "Child 可以继承 provider、审批、sandbox 和 cwd，再叠加角色配置；但 a1、a2 本身不表达业务层级。", `<div class="tree"><b>Parent</b><span>a1 · repo audit</span><span>a2 · protocol review</span><span>a3 · tests</span></div>`+p("这些任务名字和依赖关系要由 Parent 自己保留。")),
s("v1-lifecycle", "Codex V1", "V1 生命周期：创建、运行、等待、恢复、完成、关闭", "完成一轮和关闭整个 Child 不是一回事。Parent 需要区分等待状态、最终结果和生命周期回收。", flow("Spawned", "Running", "Waiting", "Running", "Completed", "Closed")),
s("v1-sequence", "Codex V1", "一次 V1 时序：先创建实例，再围绕 ID 交互", "Parent 提供任务，运行时创建 Child 并返回 ID；Child 搜索、执行并返回，Parent 用 ID 等待并整合。", `<ol class="steps"><li>Parent 调用 spawn</li><li>AgentControl 创建 Child thread</li><li>返回 agent_id</li><li>Parent wait/send/resume</li><li>Child final</li><li>Parent 验收并 close</li></ol>`),
s("v1-summary", "Codex V1", "V1 的优势与限制：实例控制清楚，协作组织依赖 Parent", "V1 很适合显式控制少量 Child；随着任务增加，Parent 要在上下文里维护谁是谁、谁完成、谁需要追问。", summary("V1 已经是真正的 Sub-Agent 生命周期，不是“不支持多 Agent”。", "不能把 V2 的出现解释成 V1 无法创建 Child；变化主要发生在协作建模。"),[],C.summary),

s("transition-chapter", "V1 → V2", "第九章导览：为什么从实例集合走向协作任务树", "V1 的问题不是不能运行 Child，而是任务身份、层级、消息和追加工作没有成为一等协作对象。", cards(["V1 中心", "Agent instance / ID。"],["V2 中心", "Task path / mailbox / follow-up。"],["转换目标", "让任务可寻址、可分层、可继续。"]),[],`${C.intro} v1-to-v2`),
s("v1-memory", "V1 → V2", "V1 管实例，协作语义由 Parent 记忆", "a1 只是句柄。它不天然告诉系统自己是 /root/repo_audit，也不建立与兄弟任务的规范通信关系。", cards(["Parent 记账", "ID → 任务、依赖、状态。"],["上下文负担", "任务越多，组织信息越挤占主线程。"],["恢复困难", "后续工作需要重新解释实例身份。"])),
s("instance-vs-task", "V1 → V2", "实例集合与任务树的差别", "实例集合回答“哪些 Child 在跑”；任务树还回答“它们属于哪个目标、路径是什么、谁可以给谁发消息、后续工作落到哪里”。", table(["问题","实例集合","任务树"],[["身份","agent id","canonical task path"],["层级","Parent 自己记","路径显式表达"],["通信","围绕实例输入","mailbox 定向投递"],["追加工作","resume/send","follow-up 语义"]])),
s("v2-goals", "V1 → V2", "V2 的设计目标：让协作本身可建模", "V2 把任务名、角色、历史继承、mailbox、follow-up 和活动状态组合成可寻址生命周期。", flow("语义选角", "创建 task path", "Child thread", "mailbox", "follow-up", "结果回收")),
s("v1-v2-compare", "V1 → V2", "V1/V2 概括对照：不是替换能力，而是提升抽象层", "V2 保留创建 Child 的核心能力，同时增加结构化任务身份和协作语义；代价是工具 schema、消息类型和上下文继承更复杂。", table(["维度","V1","V2"],[["身份","agent id","canonical path"],["组织","实例集合","任务树"],["后续","send/resume","message/follow-up"],["角色","可叠加配置","agent_type 语义选择"],["互操作","较直接消息路径","reserved schema + agent_message"]])+summary("V2 针对规模化协作组织，而不是单纯增加模型参数。", "不能把 V2 一概写成速度更快或第三方更容易；它反而增加互操作边界。"),[],C.summary),

s("v2-chapter", "Codex V2", "第十章导览：V2 是以协作任务为中心的运行模型", "V2 中 Child 不只是一个 ID，而是有 task path、角色、mailbox、历史继承策略和可继续生命周期的任务。", cards(["任务身份", "task_name → canonical path。"],["角色身份", "agent_type → custom role。"],["上下文", "fork_turns → none/all/N。"],["协作", "message 与 follow-up 分离。"]),[],`${C.intro} v2-overview`),
s("v2-task-mailbox", "Codex V2", "Task path、mailbox 与 follow-up 让任务可寻址、可继续", "task path 表达协作树身份；mailbox 投递消息；follow-up 在既有任务上启动新一轮。三者解决的是组织问题。", flow("/root", "/root/research", "mailbox message", "follow-up turn", "result")+table(["机制","解决的问题","明确不负责"],[["task path","任务在协作树中的规范身份","不选择 Role 或模型"],["mailbox","向已存在任务定向投递事实","默认不启动新一轮工作"],["follow-up","在既有任务身份上追加一轮工作","不新建另一个 canonical task"]])+p("三者把任务身份、消息投递和追加执行分开，避免 Parent 用一个模糊的 send 同时表达三种意图。")),
s("v2-role-catalog", "Codex V2", "Role catalog 与能力描述：给 Parent 一张可理解的岗位表", "Role 同时包含选择前的 description、选择后的 developer instructions 和运行时 model/provider/effort 配置。", table(["阶段","字段","作用"],[["选择前","description","告诉 Parent 何时使用"],["执行中","developer_instructions","约束 Child 行为与范围"],["运行时","model/provider/effort","绑定实际执行能力"]])+source("角色描述是 guidance，不是确定性路由；研究支持能力差异的重要性",["P04","P19","P26"]),["P04","P19","P26"]),
s("v2-args", "Codex V2", "task_name、agent_type 与 fork_turns 进入三个不同机制", "task_name 建路径，agent_type 加载角色，fork_turns 决定继承多少父历史。混用它们会让“看似传了模型”却没有真正选角。", table(["参数","回答的问题"],[["task_name","这个协作任务叫什么、位于哪条路径？"],["agent_type","使用哪一个能力角色？"],["fork_turns","Child 看见父线程的哪些历史？"],["model/effort","Child 运行绑定如何覆盖？"]])),
s("v2-six-layers", "Codex V2", "原生 V2 六层架构：配置、语义、工具、线程、运行、消息", "一次 spawn 会穿过六层。某一层文件存在，不代表后续层真的成功；TOML 只能证明配置存在，不能证明 Child 已运行。", `<div class="layers">${["配置层：catalog / feature / role","语义调度：description → role","工具契约：namespace / reserved schema","线程协作：path / mailbox / activity","运行绑定：model / provider / sandbox","消息层：Responses / agent_message / encryption"].map((x,i)=>`<div><b>0${i+1}</b>${x}</div>`).join("")}</div>`),
s("v2-sequence", "Codex V2", "原生 V2 时序：语义选角先发生，Provider 请求后发生", "Parent 先决定 agent_type，handler 再应用角色和运行覆盖、创建任务路径与 Child；随后模型请求才进入 Provider。", `<ol class="steps"><li>Parent 读取可用 Role description</li><li>调用 spawn_agent(task_name, agent_type, fork_turns)</li><li>应用 role/config/runtime override</li><li>创建 canonical path 与 Child thread</li><li>包装协作正文为 provider input</li><li>Child 执行并回到 Parent</li></ol>`),
s("v2-summary", "Codex V2", "V2 的改进与代价：协作更可描述，互操作更严格", "V2 让任务树、消息和追加工作成为一等对象，也让工具 schema、密文、私有消息和跨 transport 历史成为新的兼容面。", summary("V2 改善任务身份、层级、定向通信和继续生命周期。", "不能由 role 文件存在证明选角或请求成功，也不能假设第三方 Responses 理解 Codex 私有 item。"),[],C.summary),

s("third-party-chapter", "第三方模型障碍", "第十一章导览：第三方失败不是一个报错，而是四层漏斗", "V2 任务可能在工具 schema、任务明文、消息类型或历史回放任一层失败。先定位层次，才能避免在错误位置打补丁。", cards(["工具层", "server-reserved schema。"],["正文层", "encrypted collaboration payload。"],["消息层", "private agent_message item。"],["历史层", "Responses/Chat/Anthropic 回放差异。"]),[],C.intro),
s("failure-funnel", "第三方模型障碍", "四层失败漏斗：请求越往后，问题越具体", "第一层甚至在模型推理前拒绝；第二层拿不到任务正文；第三层第三方不认识 item；第四层多轮历史无法安全转换。", `<div class="funnel"><div style="--w:100%">1. Reserved tool schema</div><div style="--w:84%">2. Encrypted task body</div><div style="--w:68%">3. agent_message projection</div><div style="--w:52%">4. Cross-transport replay</div></div>`),
s("reserved-ciphertext", "第三方模型障碍", "Reserved schema 与 ciphertext：两个看似相邻、实际不同的边界", "给保留工具增字段会改变服务端预期 schema；即使工具名避开保留空间，正文仍可能是第三方无法解密的 ciphertext。", table(["现象","失败位置","正确判断"],[["schema mismatch","模型思考前","工具契约不一致"],["opaque payload","协作正文","第三方没有解密能力"],["空任务发送","兼容层","必须 fail closed"]])+p("CCSM 不解密 OpenAI ciphertext。")),
s("agent-message-replay", "第三方模型障碍", "agent_message 与跨 transport 回放：明文也不等于兼容", "Codex V2 可以用私有 agent_message 表达 Parent→Child；第三方 Responses、Chat 或 Anthropic 不一定接受这种 item，历史中的 reasoning/tool id 也有不同约束。", cards(["Responses", "结构化 input item。"],["Chat", "role/content messages + tools。"],["Anthropic", "不同 content blocks 与 tool use 语义。"])),
s("obstacle-summary", "第三方模型障碍", "本章结论：必须按失败层修复，不能全局改写所有消息", "工具 schema、明文、私有 item 和历史转换分别属于不同责任层。把 Official 流量也一起改掉，会破坏原生语义。", summary("先确定真实目标 Provider，再做最小、请求级、可回退的投影；opaque ciphertext 必须拒绝。", "不能声称改了工具名就解决全部问题，也不能声称 CCSM 能解密 OpenAI ciphertext。"),[],C.summary),

s("ccsm-control", "CCSM 实现", "CCSM 控制面：把用户能力意图编译为 Codex 可发现的 Role", "用户选擅长、排除、优先级、模型和推理强度；后端编译成 description、instructions 与统一 Router 绑定，再投影到配置和角色文件。", flow("能力问卷", "schema-v1 profile", "capability compiler", "agents/*.toml", "catalog/config projection", "Codex 发现")+p("CCSM 不接管 Parent 的语义选角；它提供可被选择的岗位说明与运行绑定。")),
s("provider-binding", "CCSM 实现", "Provider 物化与可变 Role 模型绑定：Role 先绑定 Router，运行时再找到真实上游", "Role 不能静态写死某个第三方 endpoint，因为同一模型名可能按当前方案、规则和认证落到不同 Provider。", flow("role.model", "codex_model_router_v2", "route match", "targetProviderId", "URL + auth + apiFormat", "real request")+source("MAS routing 研究也把协作模式、角色和模型视为联合决策",["P24"]),["P24"]),
s("ccsm-full-sequence", "CCSM 实现", "CCSM 两阶段数据面：先让任务可投递，再只对真实第三方做投影", "Stage A 在 mixed-router 非保留工具上保留可投递明文；Provider 物化后，Stage B 才把 plaintext agent_message 投影成目标协议。Official→Official 完全保留原生语义。", `<ol class="steps"><li>Parent 语义选择 Role</li><li>V2 创建 Child task</li><li>mixed-router 标记请求级明文协作</li><li>路由命中并物化真实 Provider</li><li>仅第三方 Responses 执行 agent_message → user message</li><li>需要时继续转 Chat/Anthropic</li><li>Child 结果回到 Codex 与 Parent</li></ol>`+summary("CCSM 补齐控制面配置和数据面边界，不重写 Codex orchestrator。", "TOML、构建或路由配置存在都不能替代真实 Child rollout、Provider 与 HTTP 证据。"),[],C.summary),
s("book-conclusion", "结论", "全书结论：Sub-Agent 的价值来自有条件的委托，不来自数量神话", "当任务可委托、可隔离、可验收，而且并行、互补或上下文治理收益大于通信与错误传播成本时，Sub-Agent 才有系统价值。", `<blockquote>任务结构 → 委托契约 → 组织拓扑 → 能力选角 → 过程验证 → 成本与风险验收</blockquote>`+p("Codex V1 以实例为中心，V2 以协作任务为中心；CCSM 在不接管 orchestrator、不解密 ciphertext 的前提下，通过能力编译、统一 Router、Provider 物化和最小消息投影，让第三方模型有机会成为真实 V2 Child。")+source("26 篇核心论文、OpenAI/Codex 官方证据、CCSM 源码与运行边界", catalog.papers.map(x=>x.id)),catalog.papers.map(x=>x.id),C.summary),
];

// 这些主题已完整合并进相邻的定义、案例或总结页；移除独立重复页以维持 68 页阅读节奏。
for (const id of ["harness-tools-state", "single-agent-case", "goal-drift", "capability-conflict", "cooperate-compete", "topology-information", "model-to-system", "v1-memory"]) {
  const index = slides.findIndex((slide) => slide.id === id);
  if (index < 0) throw new Error(`missing merged slide ${id}`);
  slides.splice(index, 1);
}
if (slides.length !== 68) throw new Error(`expected 68 slides, got ${slides.length}`);
const paperById = Object.fromEntries(catalog.papers.map((paper) => [paper.id, paper]));
for (const slide of slides) for (const id of slide.papers) if (!paperById[id]) throw new Error(`${slide.id}: unknown paper ${id}`);

const index = `<!DOCTYPE html>
<html lang="zh-CN" data-themes="engineering-whiteprint,academic-paper,tokyo-night">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Codex V2 Sub-Agent 学术专著</title>
<link rel="stylesheet" href="assets/fonts.css"><link rel="stylesheet" href="assets/base.css"><link rel="stylesheet" id="theme-link" href="assets/themes/engineering-whiteprint.css"><link rel="stylesheet" href="assets/animations/animations.css"><link rel="stylesheet" href="style.css"></head>
<body class="tpl-document-deck"><div class="deck">
${slides.map((slide, i) => `<section class="slide ${slide.cls}" data-slide-id="${slide.id}" data-title="${slide.title}" data-plain="${slide.plain.replaceAll('"','&quot;')}"><p class="kicker">${String(i + 1).padStart(2,"0")} / 68 · ${slide.chapter}</p><h${i===0?1:2}>${slide.title}</h${i===0?1:2}><div class="slide-body">${slide.body}</div><div class="deck-footer"><span>CODEX V2 SUB-AGENT · ACADEMIC READER</span><span class="slide-number" data-current="${i+1}" data-total="68"></span></div></section>`).join("\n")}
</div><div class="paper-dialog" role="dialog" aria-modal="true" aria-label="论文详情"><button class="paper-close">关闭 · Esc</button><div class="paper-body"></div></div>
<script src="assets/runtime.js"></script><script src="deck.js"></script></body></html>\n`;

const paperLite = Object.fromEntries(catalog.papers.map((x) => [x.id,{id:x.id,title:x.title,authors:x.authors,year:x.year,venue:x.venue,doi:x.doi,status:x.publication_status,question:x.research_question,method:x.method,scope:x.experiment_scope,support:x.supported_conclusion,boundary:x.non_generalizable_boundary,url:x.paper_url,pdf:x.pdf_url}]));
const deckJs = `(() => {
  const slides=[...document.querySelectorAll('.deck > .slide')];
  const plainLanguage=slides.map(slide=>slide.dataset.plain);
  if(plainLanguage.length!==slides.length)throw new Error('大白话数量与页面不一致');
  if(new Set(plainLanguage).size!==plainLanguage.length)throw new Error('每页大白话必须唯一');
  slides.forEach((slide,index)=>{const box=document.createElement('div');box.className='plain-language';box.innerHTML='<span class="plain-label">这页用大白话说</span><p>'+plainLanguage[index]+'</p>';slide.querySelector(index===0?'h1':'h2').insertAdjacentElement('afterend',box)});
  const papers=${JSON.stringify(paperLite)};
  const dialog=document.querySelector('.paper-dialog'),body=dialog.querySelector('.paper-body'),close=dialog.querySelector('.paper-close');let trigger=null;
  function openPaper(id,el){const p=papers[id];if(!p)return;trigger=el;body.innerHTML='<p class="kicker">'+p.id+' · '+p.year+' · '+p.status+'</p><h2>'+p.title+'</h2><p><b>作者：</b>'+p.authors.join('；')+'</p><div class="paper-grid"><div><b>研究问题</b><p>'+p.question+'</p></div><div><b>方法与范围</b><p>'+p.method+p.scope+'</p></div><div><b>支持的结论</b><p>'+p.support+'</p></div><div><b>不能外推</b><p>'+p.boundary+'</p></div></div><p class="paper-links"><a href="'+p.url+'">ACL 论文页</a><a href="'+p.pdf+'">官方 PDF</a><a href="https://doi.org/'+p.doi+'">DOI</a></p>';dialog.classList.add('open');close.focus()}
  function closePaper(){dialog.classList.remove('open');body.innerHTML='';if(trigger)trigger.focus()}
  document.querySelectorAll('[data-paper]').forEach(el=>el.addEventListener('click',()=>openPaper(el.dataset.paper,el)));close.addEventListener('click',closePaper);document.addEventListener('keydown',e=>{if(e.key==='Escape'&&dialog.classList.contains('open')){e.stopImmediatePropagation();closePaper()}},true);
})();\n`;
fs.writeFileSync(path.join(deckDir,"index.html"),index,"utf8");
fs.writeFileSync(path.join(deckDir,"deck.js"),deckJs,"utf8");
console.log(`wrote ${slides.length} slides`);
