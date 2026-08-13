# 2025–2026 Sub-Agent 与 Multi-Agent 注释书目

> 本书目是 `subagent-multiagent-2025-2026-papers.json` 的可读展开。26 篇核心论文全部是 ACL Anthology 正式论文，其中 2026 年 16 篇、2025 年 10 篇。研究结论只在各自实验范围内使用。

## 检索与纳入方法

- Codex 内置 Web Search 成功召回 ACL Anthology、arXiv 与相关官方资料；最终元数据逐篇回到 ACL 原始页面核对。
- Matrix WebSearch MCP 使用独立查询，但本轮对 2026 ACL multi-agent 的学术召回不足，主要返回日历等无关结果，因此只记录检索差异，不作为论文正证据。
- 2024 年及更早材料不计入最低数量；预印本可作延伸阅读，但不用于本目录的 26 篇核心计数。

## 主题地图

| 问题 | 代表论文 | 阅读结论 |
|---|---|---|
| 为什么拆分 | P01、P08、P10 | 可分片上下文、可合并搜索和专业化流水线能产生收益 |
| 为什么不能盲目加 Agent | P06、P09、P12、P13 | 协调鸿沟、同质辩论、冗余节点和多样性坍缩会抵消收益 |
| 如何组织 | P03、P17、P18、P22 | 拓扑、角色顺序和上下文可见性共同决定信息流 |
| 如何评价 | P02、P05、P11、P14、P15、P21 | 评价对象是系统与过程，不是只有模型和最终准确率 |
| 能力描述为何重要 | P04、P19、P20、P26 | 伙伴能力、角色差异、心智推断和团队多样性影响选角与协作 |

## [P01] Scaling External Knowledge Input Beyond Context Windows of LLMs via Multi-Agent Collaboration

- **作者：** Zijun Liu；Zhennan Wan；Peng Li；Ming Yan；Fei Huang；Yang Liu
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2026，10284–10314；DOI：[10.18653/v1/2026.acl-long.468](https://doi.org/10.18653/v1/2026.acl-long.468)；状态：正式发表。
- **研究问题：** 当外部知识超过单模型上下文窗口时，多 Agent 是否能分片处理并有效整合？
- **方法与范围：** ExtAgents 将检索知识分配给并行 Agent，再进行层级整合，并在多跳问答、长综述生成等任务比较非训练型基线。增强的多跳问答 ∞Bench+、公开测试集与长综述生成；重点考察输入位于或超过上下文窗口时的知识整合。
- **本材料可用结论：** 当输入天然可分片且结果可层级汇总时，多 Agent 可以把上下文容量问题转化为分布式知识处理问题。
- **不可外推：** 结论只适用于论文测试的知识密集任务和编排方式，不能证明任意长任务或任意 Agent 数量都会受益。
- **材料映射：** 报告 §3.1、§4.2；课件第 11、17、29 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-long.468/) · [PDF](https://aclanthology.org/2026.acl-long.468.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-long.468.pdf` · SHA-256 `2d70a04365fefe725466b3e628273b09c07ab4567cb386a0f304dc3310098c87`。

## [P02] Identifying Collective Intelligence Factor in LLM Agent Groups for Generalizable Multi-Agent System Design

- **作者：** Zhilun Zhou；Zihan Liu；Jiahe Liu；Yihan Wang；Qingyu Shao；Fengli Xu；Depeng Jin；Yong Li
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，12827–12842；DOI：[10.18653/v1/2026.findings-acl.624](https://doi.org/10.18653/v1/2026.findings-acl.624)；状态：正式发表。
- **研究问题：** LLM Agent 群体是否存在可预测跨任务表现的集体智能因子？
- **方法与范围：** 构造 108 个不同规模、模型组成与通信拓扑的 Agent 群体，跨任务测量并提取人工集体智能因子。108 个群体，多种规模、模型组合、通信拓扑和任务，用于群体泛化能力预测。
- **本材料可用结论：** 评价多 Agent 时需要把团队组成和拓扑视为系统变量，不能只看单个模型分数。
- **不可外推：** 一个统计因子能预测所测任务，不等于存在适用于所有环境的通用 Agent 总分。
- **材料映射：** 报告 §6.4、§7.2；课件第 35、40、41 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.624/) · [PDF](https://aclanthology.org/2026.findings-acl.624.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.624.pdf` · SHA-256 `5e988bc21fcbbef5f8164017de24abc4a34862dd5b5e856e18fdf1d4d3082638`。

## [P03] Dynamic Generation of Multi LLM Agents Communication Topologies with Graph Diffusion Models

- **作者：** Eric Hanchen Jiang；Levina Li；Frank Wan；Xiao Liang (梁霄)；Sophia Yin；Yuchen Wu；Xinfeng Li；Yizhou Sun；Wei Wang；Kai-Wei Chang；Ying Nian Wu
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2026，38042–38060；DOI：[10.18653/v1/2026.acl-long.1764](https://doi.org/10.18653/v1/2026.acl-long.1764)；状态：正式发表。
- **研究问题：** 能否按任务动态生成兼顾质量、通信成本与鲁棒性的 Agent 拓扑？
- **方法与范围：** Guided Topology Diffusion 用离散图扩散逐步生成拓扑，并由轻量代理模型引导多目标奖励。多个推理 benchmark，比较静态、手工与生成式拓扑的任务表现、成本和鲁棒性。
- **本材料可用结论：** 通信拓扑是可优化的系统参数，复杂任务与简单任务可能需要不同稀疏度和结构。
- **不可外推：** 论文结果不能推出动态拓扑在任意领域都优于人工层级，也不能直接映射为 Codex 的固定 Parent–Child 设计。
- **材料映射：** 报告 §6.3、§6.6；课件第 29、34、35 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-long.1764/) · [PDF](https://aclanthology.org/2026.acl-long.1764.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-long.1764.pdf` · SHA-256 `880f4e60d3938616c4574b85b7eb1eacfd1b3fa913a21884dbc159c37168ee60`。

## [P04] Explicit Trait Inference for Multi-Agent Coordination

- **作者：** Suhaib Abdurahman；Etsuko Ishii；Katerina Margatina；Divya Bhargavi；Monica Sunkara；Yi Zhang
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2026，1670–1704；DOI：[10.18653/v1/2026.acl-long.77](https://doi.org/10.18653/v1/2026.acl-long.77)；状态：正式发表。
- **研究问题：** 显式推断协作者的能力与可信度，能否减少多 Agent 协调失败？
- **方法与范围：** Explicit Trait Inference 从交互历史推断 warmth 与 competence 两类特征，用结构化伙伴画像指导决策。经济博弈与 MultiAgentBench；相对 CoT 基线测量收益损失和任务表现。
- **本材料可用结论：** 协作需要对伙伴能力形成可更新描述；只有角色名字而没有可观察能力，难以支持稳健选角。
- **不可外推：** 心理学维度和特定 benchmark 的增益不能外推为所有 Role 描述都会改善协调。
- **材料映射：** 报告 §7.1、§7.3；课件第 37、38、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-long.77/) · [PDF](https://aclanthology.org/2026.acl-long.77.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-long.77.pdf` · SHA-256 `e6a682287d1c08c55857748156597e8a0558bb97df892cf851b35a332e7a4f26`。

## [P05] MASEval: Extending Multi-Agent Evaluation from Models to Systems

- **作者：** Cornelius Emde；Alexander Rubinstein；Anmol Goel；Ahmed Heakl；Sangdoo Yun；Seong Joon Oh；Martin Gubri
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 3: System Demonstrations)，2026，345–356；DOI：[10.18653/v1/2026.acl-demo.34](https://doi.org/10.18653/v1/2026.acl-demo.34)；状态：正式发表。
- **研究问题：** 为什么多 Agent 评价必须从模型扩展到完整系统？
- **方法与范围：** MASEval 把框架、拓扑、编排、上下文工程与错误处理作为一等实验变量。3 个 benchmark、3 个模型和 3 个框架的系统级比较。
- **本材料可用结论：** 框架选择可能像模型选择一样显著影响结果，因此 Agent 评价对象应包括 harness 和 orchestration。
- **不可外推：** 系统演示论文的比较规模有限，不能给所有框架做永久排名。
- **材料映射：** 报告 §7.4、§7.5；课件第 40、41、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-demo.34/) · [PDF](https://aclanthology.org/2026.acl-demo.34.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-demo.34.pdf` · SHA-256 `894d35d4c6b8f34bf83ed0ff677e467b062d60b911798bccdc48c7dd1a31f70a`。

## [P06] SILO-BENCH: A Scalable Environment for Evaluating Distributed Coordination in Multi-Agent LLM Systems

- **作者：** Yuzhe Zhang；Feiran Liu；Yi Shan；Xinyi Huang；Xin Yang；Yueqi Zhu；Xuxin Cheng；Cao Liu；Ke Zeng；Terry Jingchen Zhang；Wenyuan Jiang
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2026，29379–29398；DOI：[10.18653/v1/2026.acl-long.1354](https://doi.org/10.18653/v1/2026.acl-long.1354)；状态：正式发表。
- **研究问题：** Agent 各自只见局部信息时，能否仅靠自由通信完成分布式计算？
- **方法与范围：** SILO-BENCH 用无预设角色的精确答案任务，系统改变协议、Agent 数量和模型。30 个算法任务、3 个通信复杂度等级、54 种配置、1,620 次实验。
- **本材料可用结论：** 高通信密度不等于有效协调；任务复杂度和 Agent 数量上升时可能出现 communication–reasoning gap。
- **不可外推：** 零成功率结论限定于 Level-III、超过 50 Agent 等实验条件，不能说所有小规模层级 Sub-Agent 都会失败。
- **材料映射：** 报告 §3.4、§6.5、§7.6；课件第 15、33、39、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-long.1354/) · [PDF](https://aclanthology.org/2026.acl-long.1354.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-long.1354.pdf` · SHA-256 `8526f19fe3dc2e2a48c23b3e8498763a9d46538d6e6b24d9f7a62ad3abd5c4d6`。

## [P07] AgentAsk: Multi-Agent Systems Need to Ask

- **作者：** Bohan Lin；Kuo Yang；Zelin Tan；Yingchuan Lai；Chen Zhang；Guibin Zhang；Xinlei Yu；Miao Yu；Xu Wang；Yudong Zhang；Yang Wang
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2026，28055–28077；DOI：[10.18653/v1/2026.acl-long.1294](https://doi.org/10.18653/v1/2026.acl-long.1294)；状态：正式发表。
- **研究问题：** 多 Agent 在消息交接处为什么失败，澄清机制能否阻断错误级联？
- **方法与范围：** 提出 Data Gap、Signal Corruption、Referential Drift、Capability Gap 四类边级错误，并用 AgentAsk 在关键交接处请求最小澄清。5 个 benchmark；同时衡量准确率、延迟和额外成本。
- **本材料可用结论：** Parent–Child 委托需要清晰的输入、输出和歧义处理机制，通信边本身也是故障面。
- **不可外推：** 最高 4.69% 增益和低于 10% 开销只属于论文实验，不可写成通用保证。
- **材料映射：** 报告 §4.3、§7.6；课件第 18、19、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-long.1294/) · [PDF](https://aclanthology.org/2026.acl-long.1294.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-long.1294.pdf` · SHA-256 `75e8a924c7ab64b9d1c5c18545b5c6fd4343571d2fc3a7189d7a2e9c60fdc766`。

## [P08] Multi-LLM Collaborative Search for Complex Problem Solving

- **作者：** Sen Yang；Yafu Li；Wai Lam；Yu Cheng
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，42599–42614；DOI：[10.18653/v1/2026.findings-acl.2115](https://doi.org/10.18653/v1/2026.findings-acl.2115)；状态：正式发表。
- **研究问题：** 多模型独立探索与迭代汇总能否扩大复杂推理的搜索空间？
- **方法与范围：** MOSA 以 MCTS 为骨架，让多个 LLM 提议、聚合并继续细化推理步骤。4 个数学与常识推理 benchmark，比较单 Agent 与其他多 Agent 基线。
- **本材料可用结论：** 当子问题允许独立探索并可通过统一搜索状态合并时，多 Agent 能提供互补候选路径。
- **不可外推：** 搜索型 benchmark 的改进不能证明软件工程、工具执行或强共享状态任务同样受益。
- **材料映射：** 报告 §3.3、§5.3；课件第 13、21、34 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.2115/) · [PDF](https://aclanthology.org/2026.findings-acl.2115.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.2115.pdf` · SHA-256 `a0841bb2402562932c2970c69b5e7d8ae4cf69300b3fdb4e5e4b7291e3c98ba6`。

## [P09] Demystifying Multi-Agent Debate: The Role of Confidence and Diversity

- **作者：** Xiaochen Zhu；Caiqi Zhang；Yizhou Chi；Tom Stafford；Nigel Collier；Andreas Vlachos
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，33909–33930；DOI：[10.18653/v1/2026.findings-acl.1694](https://doi.org/10.18653/v1/2026.findings-acl.1694)；状态：正式发表。
- **研究问题：** 为什么同质 Multi-Agent Debate 可能不如多数投票？
- **方法与范围：** 理论分析均匀信念更新，并引入多样化初始化和显式置信度通信。6 个推理型问答 benchmark，对比 vanilla debate、多数投票和两种干预。
- **本材料可用结论：** 协作收益来自有效差异和可校准信息，而不是重复启动多个同质 Agent。
- **不可外推：** 经改造 debate 的结果不能外推为所有辩论拓扑优于 Parent–Child，也不能把置信度当真实概率。
- **材料映射：** 报告 §5.4、§6.4；课件第 21、33、39 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.1694/) · [PDF](https://aclanthology.org/2026.findings-acl.1694.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.1694.pdf` · SHA-256 `c4dec55da73771f013a6d09a4b7c1626d11bbcc6b8057138efdd29812ce3bdc9`。

## [P10] LLM Multi-Agent Systems for Long Triple Set Data-to-Text Generation

- **作者：** Chinonso Cynthia Osuji；Simon Mille；Mark Andrade；Jane Adkins；Ornait O’Connell；Elaine Uí Dhonnchadha；Bláithín Heffernan；Fírinne Nic an tSaoir；Anja Belz；Thiago Castro Ferreira；Brian Davis
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，34261–34275；DOI：[10.18653/v1/2026.findings-acl.1712](https://doi.org/10.18653/v1/2026.findings-acl.1712)；状态：正式发表。
- **研究问题：** 把长结构化输入拆给专业 Agent 是否比单工作器更连贯？
- **方法与范围：** 内容排序、文本结构和表层实现由三个专业工作器完成，orchestrator 与 guardrail 闭环监督。最长 199 个 DBpedia triples，英语和爱尔兰语；人工与 LLM judge 双重评价。
- **本材料可用结论：** 可验证的专业化流水线可改善长输入生成中的控制与连贯性，但增益可能较小且依赖任务结构。
- **不可外推：** 爱尔兰语评审相关性不显著且人类与 LLM judge 对齐很弱，不能宣称多 Agent 普遍提升生成质量。
- **材料映射：** 报告 §4.4、§6.2；课件第 22、32、38 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.1712/) · [PDF](https://aclanthology.org/2026.findings-acl.1712.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.1712.pdf` · SHA-256 `af6333f9db12fb262bde02830ff0171e28e3cc888765f9ed7763e97c02e25dd5`。

## [P11] From Tasks to Teams: A Risk-First Evaluation Framework for Multi-Agent LLM Systems in Finance

- **作者：** Zichen Chen；Jianda Chen；Jiaao Chen；Misha Sra
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，38819–38857；DOI：[10.18653/v1/2026.findings-acl.1934](https://doi.org/10.18653/v1/2026.findings-acl.1934)；状态：正式发表。
- **研究问题：** 如何在准确率之外评价多 Agent 的协作、工具共享和真实行动风险？
- **方法与范围：** M-SAEA 用十种探针从模型、工作流、交互和系统四层生成连续风险向量与理由。金融管理、网店自动化、交易服务三类高风险任务，覆盖 6 个模型。
- **本材料可用结论：** 系统验收应包含安全、时效性、工具副作用和交互风险，不能只有任务正确率。
- **不可外推：** 金融场景探针不能直接证明其他行业的风险阈值，也不能替代人工安全审计。
- **材料映射：** 报告 §7.5、§7.6；课件第 41、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.1934/) · [PDF](https://aclanthology.org/2026.findings-acl.1934.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.1934.pdf` · SHA-256 `7ce94b8fafa8668782e2a1cd603b91ac059b2dfc89fdf81a10333a5e7d1f09a6`。

## [P12] AgentSlimming: Towards Efficient and Cost-Aware Multi-Agent Systems

- **作者：** Yulang Chen；Haoxuan Peng；Jinyan Liu；Zichen Wen；Dongrui Liu；Linfeng Zhang
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2026，30064–30086；DOI：[10.18653/v1/2026.acl-long.1387](https://doi.org/10.18653/v1/2026.acl-long.1387)；状态：正式发表。
- **研究问题：** 图结构工作流中哪些 Agent 或模型是冗余的？
- **方法与范围：** AgentSlimming 估计节点重要性，删除冗余 Agent 或替换低成本模型，并用基线锚定规则防止质量崩塌。多个图结构 multi-agent workflow，比较 token 成本与任务质量的 Pareto 关系。
- **本材料可用结论：** Agent 数量和模型成本都应按边际贡献验收；可删除的节点说明更多 Agent 并非天然更好。
- **不可外推：** 最高 78.9% token 降幅属于测试工作流，不能承诺 CCSM 或 Codex 获得同等节省。
- **材料映射：** 报告 §7.5、§7.6；课件第 39、41、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-long.1387/) · [PDF](https://aclanthology.org/2026.acl-long.1387.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-long.1387.pdf` · SHA-256 `b3acc39c06f1ac19716535ee71dfdbcc93e7a096fe097f75f0c8515cf0b05790`。

## [P13] Diversity Collapse in Multi-Agent LLM Systems: Structural Coupling and Collective Failure in Open-Ended Idea Generation

- **作者：** Nuo Chen；Yicheng Tong；Yuzhe Yang；Yufei He；Xueyi Zhang；Zou Qingyun；Qian Wang；Bingsheng He
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，251–306；DOI：[10.18653/v1/2026.findings-acl.13](https://doi.org/10.18653/v1/2026.findings-acl.13)；状态：正式发表。
- **研究问题：** 多 Agent 互动是否真的扩大开放式创意的解空间？
- **方法与范围：** 从模型能力、Agent 认知和系统动力三层分析开放式创意中的语义多样性。多模型、多团队权力结构、不同群体规模和通信密度的创意生成实验。
- **本材料可用结论：** 密集通信和权威耦合可能让 Agent 过早趋同；隔离与独立性有时比更多消息更重要。
- **不可外推：** 创意多样性结果不能直接等同于代码正确率或事实问答精度。
- **材料映射：** 报告 §3.5、§6.5；课件第 15、33、39 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.13/) · [PDF](https://aclanthology.org/2026.findings-acl.13.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.13.pdf` · SHA-256 `746caf0657ba364c45e089b6a66fd8d9f884b34817919daa318d219a0f357bd2`。

## [P14] Superficial Success vs. Internal Breakdown: An Empirical Study of Generalization in Adaptive Multi-Agent Systems

- **作者：** Namyeong So；Seokgyu Jang；Taeuk Kim
- **出版：** Findings of the Association for Computational Linguistics: ACL 2026，2026，15328–15354；DOI：[10.18653/v1/2026.findings-acl.753](https://doi.org/10.18653/v1/2026.findings-acl.753)；状态：正式发表。
- **研究问题：** 自适应 MAS 的最终正确率是否掩盖内部协调崩坏？
- **方法与范围：** 跨域转移和内部行为分析识别 topological overfitting 与 illusory coordination。多种自适应 MAS 在训练域与迁移域上的最终结果和内部协作行为。
- **本材料可用结论：** 最终答案正确不足以证明协作机制健康，必须检查任务分工、消息与回收过程。
- **不可外推：** 论文定义的理想 MAS 行为有特定度量，不能未经复核套用到所有运行时。
- **材料映射：** 报告 §7.4、§7.6；课件第 40、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-acl.753/) · [PDF](https://aclanthology.org/2026.findings-acl.753.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-acl.753.pdf` · SHA-256 `afa59dd89629f6a09867e2c9b66cf9e1b85a288405cf54ec6501fd9d16cb33d6`。

## [P15] Process Evaluation for Agentic Systems

- **作者：** Milan Gritta；Debjit Paul；Xiaoguang Li；Lifeng Shang；Jun Wang；Gerasimos Lampouras
- **出版：** Findings of the Association for Computational Linguistics: EACL 2026，2026，2678–2692；DOI：[10.18653/v1/2026.findings-eacl.140](https://doi.org/10.18653/v1/2026.findings-eacl.140)；状态：正式发表。
- **研究问题：** 只评价最终答案会漏掉哪些 Agent 风险？
- **方法与范围：** 立场论文提出过程评价与 compliance score，并用小规模研究展示自动评价可行性。医疗、金融、法律、基础设施等敏感应用讨论，加一项小规模过程评价研究。
- **本材料可用结论：** Agent 可能通过跳步骤、伪造工具调用或使用过期知识得到表面正确答案，因此要验证过程。
- **不可外推：** 这是立场论文和小规模验证，不能当作成熟通用评价标准。
- **材料映射：** 报告 §7.4、§7.6；课件第 40、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.findings-eacl.140/) · [PDF](https://aclanthology.org/2026.findings-eacl.140.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.findings-eacl.140.pdf` · SHA-256 `79e491f1ae4918c0ca4cf4212184826c3654ebbee4aef360e9708b546897c419`。

## [P16] Towards Effective and Efficient Multi-Agent Language Model Systems: Foundations, Prospects, and Applications

- **作者：** Xuan Wang；Shuxiang Cao；Yuchen Zhuang；Wenqi Shi
- **出版：** Proceedings of the 64th Annual Meeting of the Association for Computational Linguistics (Volume 5: Tutorial Abstracts)，2026，5–6；DOI：[10.18653/v1/2026.acl-tutorials.3](https://doi.org/10.18653/v1/2026.acl-tutorials.3)；状态：正式发表。
- **研究问题：** 有效且高效的多 Agent LLM 系统由哪些研究问题组成？
- **方法与范围：** ACL 教程从单 Agent 能力、协作通信与真实应用三个部分梳理方法。教程摘要覆盖模型蒸馏、动态路由、记忆/服务效率、图优化、谄媚缓解与应用。
- **本材料可用结论：** 模型、Agent、通信和服务成本是一个耦合系统，不能只改一个层面后宣布整体最优。
- **不可外推：** 教程摘要用于建立研究地图，不作为具体性能数字的实验证据。
- **材料映射：** 报告 §2.1、§5.1；课件第 4、24、43 页。
- **原文与归档：** [论文页](https://aclanthology.org/2026.acl-tutorials.3/) · [PDF](https://aclanthology.org/2026.acl-tutorials.3.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2026.acl-tutorials.3.pdf` · SHA-256 `60fdf7456fb8bbb05ccef5c88b1ba1f4987a517f5df77164fcaf0333c630cb33`。

## [P17] MultiAgentBench : Evaluating the Collaboration and Competition of LLM agents

- **作者：** Kunlun Zhu；Hongyi Du；Zhaochen Hong；Xiaocheng Yang；Shuyi Guo；Daisy Zhe Wang；Zhenhailong Wang；Cheng Qian；Xiangru Tang；Heng Ji；Jiaxuan You
- **出版：** Proceedings of the 63rd Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2025，8580–8622；DOI：[10.18653/v1/2025.acl-long.421](https://doi.org/10.18653/v1/2025.acl-long.421)；状态：正式发表。
- **研究问题：** 如何同时衡量 LLM Agent 的协作与竞争？
- **方法与范围：** MultiAgentBench 在交互场景中以里程碑 KPI 比较星、链、树、图和认知规划等协议。多种协作/竞争场景与多种协调协议；报告任务得分和里程碑达成。
- **本材料可用结论：** 拓扑改变会改变任务表现，系统评价需要过程里程碑，而非只看最终完成。
- **不可外推：** 某研究场景中图结构最好、规划提高 3% 不代表所有场景的全局最优。
- **材料映射：** 报告 §5.2、§6.1、§7.4；课件第 25、29、40 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.acl-long.421/) · [PDF](https://aclanthology.org/2025.acl-long.421.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.acl-long.421.pdf` · SHA-256 `035f673b459aecad0b7fa0fdac21dd9585f644cb29fba45f4ecbceef28f81eb5`。

## [P18] Understanding the Information Propagation Effects of Communication Topologies in LLM-based Multi-Agent Systems

- **作者：** Xu Shen；Yixin Liu；Yiwei Dai；Yili Wang；Rui Miao；Yue Tan；Shirui Pan；Xin Wang
- **出版：** Proceedings of the 2025 Conference on Empirical Methods in Natural Language Processing，2025，12347–12361；DOI：[10.18653/v1/2025.emnlp-main.623](https://doi.org/10.18653/v1/2025.emnlp-main.623)；状态：正式发表。
- **研究问题：** 稀疏和稠密拓扑如何传播正确信息与错误？
- **方法与范围：** 构建因果分析框架，并提出融合稀疏/稠密连接的 EIB-Learner。多种拓扑稀疏度下比较正确/错误输出传播、通信成本和鲁棒性。
- **本材料可用结论：** 中等稀疏结构可能在传播有益信息与抑制错误之间取得更好平衡。
- **不可外推：** 最优稀疏度依赖任务和模型，不能写成固定边数规则。
- **材料映射：** 报告 §6.1、§6.5；课件第 29、34、35 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.emnlp-main.623/) · [PDF](https://aclanthology.org/2025.emnlp-main.623.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.emnlp-main.623.pdf` · SHA-256 `f94767d936354030dc25f10db92a2f6f85f49b7d7163ac45b253e047ca67bd8b`。

## [P19] Advancing Collaborative Debates with Role Differentiation through Multi-Agent Reinforcement Learning

- **作者：** Haoran Li；Ziyi Su；Yun Xue (薛云)；Zhiliang Tian；Yiping Song；Minlie Huang
- **出版：** Proceedings of the 63rd Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2025，22655–22666；DOI：[10.18653/v1/2025.acl-long.1105](https://doi.org/10.18653/v1/2025.acl-long.1105)；状态：正式发表。
- **研究问题：** 角色如何从标签变成可学习的互补行为？
- **方法与范围：** Multi-LLM Cooperation 联合学习时序角色嵌入，并用角色差异模块避免收敛。7 个数据集上的协作与专业性实验。
- **本材料可用结论：** 有效角色需要表达任务相关的能力差异；通用的 judge/summarizer 名字未必足够。
- **不可外推：** 学习式角色嵌入的结果不能证明自然语言 description 一定产生相同互补性。
- **材料映射：** 报告 §7.1、§7.2；课件第 37、38、39 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.acl-long.1105/) · [PDF](https://aclanthology.org/2025.acl-long.1105.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.acl-long.1105.pdf` · SHA-256 `ddeebe36c83773a78d815c2b811f6634594ca2bfe1d2288b40a9c26d1cbc2775`。

## [P20] LLM-Coordination: Evaluating and Analyzing Multi-agent Coordination Abilities in Large Language Models

- **作者：** Saaket Agashe；Yue Fan；Anthony Reyna；Xin Eric Wang
- **出版：** Findings of the Association for Computational Linguistics: NAACL 2025，2025，8053–8072；DOI：[10.18653/v1/2025.findings-naacl.448](https://doi.org/10.18653/v1/2025.findings-naacl.448)；状态：正式发表。
- **研究问题：** LLM 在纯协调问题中具备哪些能力、缺少哪些能力？
- **方法与范围：** LLM-Coordination 包含 4 个纯协调博弈和 198 个 CoordQA 问题，分解环境理解、Theory of Mind 与联合规划。Agentic Coordination、CoordQA 和未见伙伴的 zero-shot coordination。
- **本材料可用结论：** 环境线索驱动的协调相对较强，但需要推断伙伴信念与意图时仍有明显不足。
- **不可外推：** 纯协调游戏不能完整代表软件工程 Parent–Child 生命周期。
- **材料映射：** 报告 §5.3、§7.2；课件第 26、38、41 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.findings-naacl.448/) · [PDF](https://aclanthology.org/2025.findings-naacl.448.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.findings-naacl.448.pdf` · SHA-256 `0990537c9643c4057464000e85509949d75266b06359d99f18732883fa6d222d`。

## [P21] Collab-Overcooked: Benchmarking and Evaluating Large Language Models as Collaborative Agents

- **作者：** Haochen Sun；Shuwen Zhang；Lujie Niu；Lei Ren；Hao Xu；Hao Fu；Fangkun Zhao；Caixia Yuan；Xiaojie Wang
- **出版：** Proceedings of the 2025 Conference on Empirical Methods in Natural Language Processing，2025，4922–4951；DOI：[10.18653/v1/2025.emnlp-main.249](https://doi.org/10.18653/v1/2025.emnlp-main.249)；状态：正式发表。
- **研究问题：** 交互环境中 Agent 能否持续理解目标、主动协作并适应伙伴？
- **方法与范围：** Collab-Overcooked 扩展 Overcooked-AI，并引入过程导向的协作指标。13 个 LLM、30 个开放任务和自然语言协作环境。
- **本材料可用结论：** 模型可能理解目标却仍缺乏主动协作与持续适应；理解任务不等于能形成团队。
- **不可外推：** 游戏环境的动作和通信约束不能直接外推到代码 Agent。
- **材料映射：** 报告 §5.3、§7.4；课件第 26、40、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.emnlp-main.249/) · [PDF](https://aclanthology.org/2025.emnlp-main.249.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.emnlp-main.249.pdf` · SHA-256 `757bb679e728dbc84bdbe81dfaca790ce63e5b53547165e094a39e17fe57d501`。

## [P22] AnyMAC: Cascading Flexible Multi-Agent Collaboration via Next-Agent Prediction

- **作者：** Song Wang；Zhen Tan；Zihan Chen；Shuang Zhou；Tianlong Chen；Jundong Li
- **出版：** Proceedings of the 2025 Conference on Empirical Methods in Natural Language Processing，2025，11555–11567；DOI：[10.18653/v1/2025.emnlp-main.584](https://doi.org/10.18653/v1/2025.emnlp-main.584)；状态：正式发表。
- **研究问题：** 能否动态选择下一位 Agent 及其可见上下文？
- **方法与范围：** AnyMAC 使用 Next-Agent Prediction 和 Next-Context Selection 构造任务自适应的顺序协作管线。多个 benchmark 上比较静态/图结构基线的表现与通信开销。
- **本材料可用结论：** 动态选角必须同时决定谁执行和看什么上下文；上下文治理与角色路由不可分割。
- **不可外推：** 顺序级联的增益不代表并行 Parent–Child 在所有任务上更差。
- **材料映射：** 报告 §6.3、§7.2；课件第 35、38、56 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.emnlp-main.584/) · [PDF](https://aclanthology.org/2025.emnlp-main.584.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.emnlp-main.584.pdf` · SHA-256 `63ebb5d31fb65f83a18c7c0791b89595ac0b45b8f36597f4cbd5c0bdf345b266`。

## [P23] Optima: Optimizing Effectiveness and Efficiency for LLM-Based Multi-Agent System

- **作者：** Weize Chen；Jiarui Yuan；Chen Qian；Cheng Yang；Zhiyuan Liu；Maosong Sun (孙茂松)
- **出版：** Findings of the Association for Computational Linguistics: ACL 2025，2025，11534–11557；DOI：[10.18653/v1/2025.findings-acl.601](https://doi.org/10.18653/v1/2025.findings-acl.601)；状态：正式发表。
- **研究问题：** 如何联合优化多 Agent 的任务效果、token 效率和通信可读性？
- **方法与范围：** Optima 采用生成、排序、选择、训练循环，比较 SFT、DPO 与混合方案，并借鉴 MCTS 生成偏好数据。信息不对称问答和复杂推理，使用 Llama 3 8B/3.2 3B 等配置。
- **本材料可用结论：** 多 Agent 的优化目标需要同时包含质量、通信成本和可读性。
- **不可外推：** 最高 2.8 倍表现与少于 10% token 只属于指定模型、任务和训练设置。
- **材料映射：** 报告 §7.5、§7.6；课件第 41、42 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.findings-acl.601/) · [PDF](https://aclanthology.org/2025.findings-acl.601.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.findings-acl.601.pdf` · SHA-256 `6f136b9b97ea7d57b9b23f952df7b0a7e994de5ba064aa35ea0415388c9120ac`。

## [P24] MasRouter: Learning to Route LLMs for Multi-Agent Systems

- **作者：** Yanwei Yue；Guibin Zhang；Boyang Liu；Guancheng Wan；Kun Wang；Dawei Cheng；Yiyan Qi
- **出版：** Proceedings of the 63rd Annual Meeting of the Association for Computational Linguistics (Volume 1: Long Papers)，2025，15549–15572；DOI：[10.18653/v1/2025.acl-long.757](https://doi.org/10.18653/v1/2025.acl-long.757)；状态：正式发表。
- **研究问题：** Multi-Agent 系统怎样联合选择协作模式、角色和模型？
- **方法与范围：** MasRouter 通过级联控制器依次确定协作模式、角色分配和 LLM 路由。代码与推理 benchmark，比较性能、成本和接入不同 MAS 框架的能力。
- **本材料可用结论：** 多 Agent 模型绑定不是单一模型路由问题，而是协作模式、角色和 provider 的联合决策。
- **不可外推：** MBPP/HumanEval 上的数字不能外推为 CCSM 的确定性语义选角或实际节省。
- **材料映射：** 报告 §7.2、§12.2；课件第 38、56、66 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.acl-long.757/) · [PDF](https://aclanthology.org/2025.acl-long.757.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.acl-long.757.pdf` · SHA-256 `1bf45eaa68515ae2a6d3de2e2240ac321fef37a46ba831718aacee52bb12f457`。

## [P25] MALLM: Multi-Agent Large Language Models Framework

- **作者：** Jonas Becker；Lars Benedikt Kaesberg；Niklas Bauer；Jan Philip Wahle；Terry Ruas；Bela Gipp
- **出版：** Proceedings of the 2025 Conference on Empirical Methods in Natural Language Processing: System Demonstrations，2025，418–439；DOI：[10.18653/v1/2025.emnlp-demos.29](https://doi.org/10.18653/v1/2025.emnlp-demos.29)；状态：正式发表。
- **研究问题：** 如何系统比较 persona、响应方式、讨论范式和决策协议？
- **方法与范围：** MALLM 提供 144 种以上可配置组合和统一评估管线。Hugging Face 文本数据集、persona、response generator、discussion paradigm 和 voting/consensus。
- **本材料可用结论：** Agent 组织不是一个开关；persona、信息流和决策协议需要分别配置和比较。
- **不可外推：** 框架可配置性本身不证明某个配置有效，也不代表 Codex V2 的官方架构。
- **材料映射：** 报告 §5.4、§6.4；课件第 33、40 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.emnlp-demos.29/) · [PDF](https://aclanthology.org/2025.emnlp-demos.29.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.emnlp-demos.29.pdf` · SHA-256 `55b1b58f23b9238ec453f3fb1103f9b26d86a7b154d543f89b15a0cb6ea6bae5`。

## [P26] AgentInit: Initializing LLM-based Multi-Agent Systems via Diversity and Expertise Orchestration for Effective and Efficient Collaboration

- **作者：** Chunhao Tian；Yutong Wang；Xuebo Liu；Zhexuan Wang；Liang Ding；Miao Zhang；Min Zhang
- **出版：** Findings of the Association for Computational Linguistics: EMNLP 2025，2025，11870–11902；DOI：[10.18653/v1/2025.findings-emnlp.636](https://doi.org/10.18653/v1/2025.findings-emnlp.636)；状态：正式发表。
- **研究问题：** 如何在任务相关性、专业性与多样性之间初始化 Agent 团队？
- **方法与范围：** AgentInit 通过多轮交互/反思生成 Agent，用格式化机制和 Pareto 团队选择平衡多样性与任务相关性。多框架、多任务，比较预定义与自动初始化策略的表现和 token。
- **本材料可用结论：** 团队组建需要同时考虑能力相关性与多样性；角色描述是系统选择能力的输入。
- **不可外推：** 最高提升数字依赖论文的初始化方法与任务，不能把 description 当确定性路由规则。
- **材料映射：** 报告 §7.1、§7.2；课件第 37、38、56 页。
- **原文与归档：** [论文页](https://aclanthology.org/2025.findings-emnlp.636/) · [PDF](https://aclanthology.org/2025.findings-emnlp.636.pdf) · 本地 `docs/references/papers/subagent-multiagent-2025-2026/2025.findings-emnlp.636.pdf` · SHA-256 `b1a207b855995ea3af241103cab79f0f7c1d1eb4c6212e5bf256fabe17c649d7`。

