(function(){
  const plainLanguage=[
    "这套材料要回答一件事：Codex 怎样把工作分给子智能体，以及 CCSM 怎样让第三方模型也能接到这份工作。后面出现英文名词时，会同时说明它在整条链路里到底负责什么。",
    "读这份材料不用先懂源码。先弄明白为什么要分工，再看 V1 和 V2 怎么分，最后再看第三方模型为什么接不到任务、CCSM 又是怎样把路修通的。",
    "一个智能体把查资料、看日志、改代码和写结论全塞进同一个聊天，容易越干越乱。子智能体的作用，就是把杂活分出去，让主智能体一直记得最终目标。",
    "主智能体（Parent Agent）像项目负责人，负责分工、把关和收尾；子智能体（Child Agent）像专项成员，只负责被分配的那一小块工作。它完成任务，不自动接管整个项目。",
    "V1 的做法像给每个临时工发一个编号。主智能体以后要追问、等待或关闭谁，都得拿着这个编号自己管理。",
    "这页展示 V1 子智能体从被创建到结束的一生。完成一轮工作不等于已经注销；主智能体还可以继续追问，也可以明确关闭它。",
    "这是一趟典型的 V1 交接过程：主智能体派任务、拿到编号、等待结果，最后自己整理成对用户负责的答案。任务是什么意思，仍主要记在主智能体脑子里。",
    "V2 不再只管一个编号，而是给每项协作工作一个正式的任务地址。这样谁在做什么、任务属于哪一层、完成后还能不能追加工作，都更清楚。",
    "V2 把“告诉你一个消息”和“请你再做一轮”分成两种动作。前者只是投递信息，后者会把已经闲下来的子智能体重新叫起来干活。",
    "V1 更像管理一群临时执行者，V2 更像管理一棵任务树。V2 更容易组织复杂协作，但也带来了更严格的角色、历史上下文和消息格式要求。",
    "一次 V2 调用要连续经过六道关：先读配置，再选角色，再校验工具格式，然后创建子线程、确定模型，最后才把消息发给模型服务。前面任何一层没过，后面都不会发生。",
    "角色配置不是一个名字那么简单。它要分别告诉主智能体“什么时候选我”、告诉子智能体“选中后怎么做”，还要告诉系统“最后让哪个模型、用什么权限来执行”。",
    "代码里的 task_name 是任务的名字，agent_type 才是要使用的角色；model 和 reasoning_effort 则决定具体模型和推理强度。它们看起来都像“选择”，实际上管的是不同事情。",
    "fork_turns 决定子智能体要不要带着主智能体以前的聊天记录上岗。带得越多，背景越完整；但想换成不同角色或不同模型时，完整继承反而可能成为限制。",
    "这页把原生 V2 的完整流水线串起来：系统先读角色，主智能体决定派谁，运行时创建子线程，模型服务完成任务，最后结果再回到主智能体。",
    "系统不能只看模型名字决定派谁，因为名字看不出它擅长查资料还是擅长复杂改造。能力描述就像岗位说明书，帮助主智能体按任务内容选择更合适的角色。",
    "任务名只是文件夹标签，角色名才决定由哪类人来做。把任务叫作 deepseek_flash，并不会自动让 DeepSeek 角色上岗；必须明确填写 agent_type。",
    "第三方模型要成为 V2 子智能体，必须连续通过四道门：工具格式能被官方接受、任务正文是可读明文、消息类型是第三方认识的格式、历史记录也能在不同接口间安全转换。",
    "有些协作工具是 OpenAI 官方预留格式，字段不能随便增删。即使只是多加一个可选参数，服务端也可能在模型开始思考之前直接拒绝整次请求。",
    "把工具换一个名字，只能避开官方保留格式的冲突；任务正文仍可能被加密，消息外壳仍可能是第三方不认识的 agent_message，历史记录也可能无法直接重放。",
    "三种报错看起来都像“子智能体没启动”，实际坏在三个不同位置：工具格式、任务明文、消息类型。只有先判断坏在哪一层，才能用对修复办法。",
    "CCSM 没有重新写一套子智能体系统。Codex 仍负责分工和回收结果；CCSM 只负责把角色配置准备好、找到真实模型服务，并把消息翻译成第三方能读的格式。",
    "控制面可以理解为“开工前的人事和配置工作”：用户填写这个角色擅长什么，CCSM 把这些选择整理成角色文件、模型目录和 Codex 配置，供主智能体选择。",
    "这段编译代码是在自动写岗位说明书。它把擅长什么、不适合什么、什么时候优先使用组合成 description，让主智能体看到的是完整能力，而不只是一个模型名。",
    "两阶段不能合并：第一阶段只负责让官方主智能体产生可投递的明文任务；第二阶段等系统确认真的要去第三方模型后，才把 Codex 内部消息转换成标准消息。",
    "路由规则只说“这个模型应该走哪条路”，还不包含完整的服务器地址和认证信息。Provider 物化就是把路由命中的结果和真实服务商配置合成一次可以真正发送的请求。",
    "如果任务已经是明文，CCSM 会把 Codex 内部的 agent_message 换成普通用户消息；如果仍是无法读取的密文，就明确报错，绝不把空任务或乱码当成成功发送。",
    "这页把数据链从头走一遍：官方主智能体先生成任务，CCSM 找到真实第三方服务并翻译消息，第三方模型完成工作，Codex 再把结果交回主智能体。",
    "创建一个新角色不是写完一个 TOML 文件就结束。它还要经过校验、重命名、配置投影、主智能体选角、路由到真实服务商，最终真的执行并返回结果。",
    "这七行配置各管一件事：名字用于识别，description 用于选角，instructions 用于约束工作，model/provider/effort 决定具体由谁、以多大推理强度执行。",
    "这些坑的共同原因，是在错误的层解决问题：用任务名代替角色、用模型名猜服务商、为了第三方修改所有官方消息。正确做法是让每一层只处理自己负责的事情。",
    "V23 修的是“系统还没开工就读配置失败”的问题：模型目录路径必须是绝对路径，旧字段和新字段也不能同时出现。配置能正常加载后，后面的选角、路由和消息转换才有机会运行。"
  ];
  const slides=[...document.querySelectorAll('.deck > .slide')];
  if(plainLanguage.length!==slides.length){throw new Error(`大白话解释数量 ${plainLanguage.length} 与页面数量 ${slides.length} 不一致`)}
  slides.forEach((slide,index)=>{
    const box=document.createElement('div');box.className='plain-language';
    box.innerHTML=`<span class="plain-label">这页用大白话说</span><p>${plainLanguage[index]}</p>`;
    const anchor=slide.querySelector(index===0?'h1':'h2');
    if(!anchor){throw new Error(`第 ${index+1} 页缺少标题，无法插入大白话解释`)}
    anchor.insertAdjacentElement('afterend',box);
  });
  const terms={
    "Sub-Agent":"由 parent 创建、在独立 Agent thread 中承担有边界任务，并把结果返回 parent 的协作执行者。",
    "context pollution":"主线程被探索日志、测试输出和中间材料挤占，导致真正决策信息难以维持。",
    "task path":"V2 协作树中的规范任务身份，例如 /root/repo_audit；它不是角色选择器。",
    "mailbox":"V2 在任务之间投递 spawn、message 与 follow-up 通信的可寻址通道。",
    "reserved schema":"由服务端模型配置保留、要求客户端严格匹配的工具 JSON Schema。",
    "agent_message":"Codex V2 用于 parent/child 协作的私有 Responses input item，不是通用 user message。",
    "Provider materialization":"用 route 命中的真实 Provider 为底座，合并认证、URL、协议、模型映射与请求级路由身份。",
    "Responses":"以 input item 表达消息、推理和工具调用的响应式 API 结构。",
    "Chat bridge":"把 Responses input/history 归一化为第三方 Chat messages 与工具结构的转换层。",
    "fork_turns":"V2 child 继承 parent 历史的策略：none、all 或最近 N 轮。",
    "description":"给 parent 的角色选择 guidance，说明何时应使用该角色；不是确定性硬路由。",
    "developer_instructions":"角色被选中后注入 child 的执行规则与范围边界。",
    "MultiRouter":"CCSM 按模型与 route 配置解析真实上游 Provider 的统一路由入口。",
    "fail closed":"遇到不可读 ciphertext 或不完整任务时明确拒绝，不把空任务或伪成功发送给上游。"
  };
  const pop=document.createElement('div');pop.className='term-popover';pop.setAttribute('role','dialog');pop.innerHTML='<button type="button" aria-label="关闭">×</button><strong></strong><span></span>';document.body.appendChild(pop);
  let pinned=false,current=null;
  function place(el){const r=el.getBoundingClientRect(),w=360,h=150;let x=Math.min(innerWidth-w-12,Math.max(12,r.left)),y=r.bottom+10;if(y+h>innerHeight-12)y=Math.max(12,r.top-h-10);pop.style.left=x+'px';pop.style.top=y+'px'}
  function open(el,pin){current=el;pinned=pin;pop.querySelector('strong').textContent=el.dataset.term;pop.querySelector('span').textContent=terms[el.dataset.term]||'';place(el);pop.classList.add('open')}
  function close(){pinned=false;current=null;pop.classList.remove('open')}
  document.querySelectorAll('.term').forEach(el=>{el.addEventListener('mouseenter',()=>{if(!pinned)open(el,false)});el.addEventListener('mouseleave',()=>{if(!pinned)close()});el.addEventListener('focus',()=>{if(!pinned)open(el,false)});el.addEventListener('blur',()=>{if(!pinned)close()});el.addEventListener('click',e=>{e.stopPropagation();open(el,true)});el.addEventListener('keydown',e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();open(el,true)}})});
  pop.querySelector('button').addEventListener('click',close);document.addEventListener('click',e=>{if(pinned&&!pop.contains(e.target)&&e.target!==current)close()});document.addEventListener('keydown',e=>{if(e.key==='Escape')close()});window.addEventListener('resize',()=>{if(current)place(current)});

  const explore=document.createElement('div');explore.className='explore-panel';explore.setAttribute('role','dialog');explore.setAttribute('aria-modal','true');explore.innerHTML='<button type="button" class="explore-close">关闭 · Esc</button><div class="explore-body"></div>';document.body.appendChild(explore);
  const exploreBody=explore.querySelector('.explore-body');let lastTrigger=null;
  function openExplore(id,trigger){const tpl=document.getElementById(id);if(!tpl)return;lastTrigger=trigger;exploreBody.innerHTML=tpl.innerHTML;explore.classList.add('open');explore.querySelector('.explore-close').focus()}
  function closeExplore(){if(!explore.classList.contains('open'))return;explore.classList.remove('open');exploreBody.innerHTML='';if(lastTrigger)lastTrigger.focus()}
  document.querySelectorAll('[data-explore]').forEach(el=>el.addEventListener('click',e=>{e.preventDefault();e.stopPropagation();openExplore(el.dataset.explore,el)}));
  explore.querySelector('.explore-close').addEventListener('click',closeExplore);document.addEventListener('keydown',e=>{if(e.key==='Escape'&&explore.classList.contains('open')){e.stopImmediatePropagation();closeExplore()}},true);
})();
