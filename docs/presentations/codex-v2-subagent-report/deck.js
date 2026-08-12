(function(){
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
})();
