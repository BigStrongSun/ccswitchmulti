# 2026-09-15 模态弹窗打开时全局通知层点不动（pointer-events 继承，不是 z-index）

## 现象

用户截图：「正在刷新 Codex 状态」弹窗打开期间，顶部黄色通知
「检测到上次运行未正常退出，请检查配置恢复结果」及其「查看日志」按钮完全点不动，
看起来像被上层容器遮挡。用户要求检查渲染层是否写错。

## 根因

- Radix 模态弹窗（`@radix-ui/react-dialog` → `@radix-ui/react-dismissable-layer`
  的 `disableOutsidePointerEvents`，见 `DismissableLayer` 源码
  `ownerDocument.body.style.pointerEvents = "none"`）会给 **document.body**
  加**内联** `pointer-events: none`，只把弹窗内容（layer node）自己标成 `auto`。
- `pointer-events` 是**继承属性**；sonner 的通知层挂在 `#root`（body 的后代）里，
  于是 `ol[data-sonner-toaster]` → `li[data-sonner-toast]` → 操作按钮整条链
  全部继承成 `none`，点击落不到任何元素。
- sonner 自身 CSS 只在 `[data-sonner-toast][data-visible='false']` 上声明
  `pointer-events: none`，对容器/可见 toast 没有声明，因此完全依赖继承值。
- 结论：不是 z-index 遮挡（toast 的 `z-index: 999999999` 一直在弹窗之上，
  视觉也压在遮罩之上），而是继承下来的 pointer-events 被关掉。

## 复现与验证方法（可复用）

`src/components/ui/sonner.test.tsx`（新增）在 jsdom 里同时渲染 Radix 模态弹窗与
`<Toaster/>`，然后：
1. 断言 `document.body.style.pointerEvents === "none"`（机制前提）；
2. 打印/断言 computed `pointer-events`：修复前
   `body / toaster / toast / button` 全是 `none`；
3. 用 `user-event` 真点「查看日志」——修复前直接抛
   `Unable to perform pointer interaction as the element has pointer-events: none`。

## 修复（v3.20.2-17，提交 2626de46）

- `src/components/ui/sonner.tsx`：给通知容器显式声明
  `style={{ pointerEvents: "auto" }}`，覆盖从 body 继承的 `none`。
- 为什么不用 `index.css` 里的 `[data-sonner-toast] { pointer-events: auto }`：
  vitest 默认不加载 CSS（`css` 未开启），规则无法回归测试；容器内联样式既确定
  又可测。
- 为什么不直接给每个 toast 内联 `pointer-events: auto`：内联优先级高于 sonner
  自己的 `[data-sonner-toast][data-visible='false'] { pointer-events: none }`，
  会让隐藏/移出中的 toast 也变得可点（可能截获本应落到前台 toast 的点击）。
- 模态弹窗对**后台应用 UI** 的封锁不受影响：只有通知层这一棵子树被显式放开。

## 验证

- `vitest src/components/ui/sonner.test.tsx`（修复前失败、修复后通过）、
  `tsc --noEmit`、Prettier、`CodexConfigConsistencyDialog` 14 passed 全绿。
- 通用教训：任何“挂在 body 下、需要在模态弹窗之上交互”的全局层，都必须显式声明
  `pointer-events`，不能依赖默认继承；排查“点不动”时先看 computed
  `pointer-events`，再看 z-index。

## 附带确认：用户 10:59 的历史修复已生效

同一轮排查里重跑了只读诊断（`cargo test --lib real_history_blocked_reason_census
-- --ignored --nocapture`）：
`affected=0 duplicate_ordinals=0 provider_cursors=0 history_base=0 blocked=38`，
剩余 4 组原因 11（父段引用偏移落在记录中间）+11（父段引用缺少可迁移前备份）
+10（会话 id 不一致）+6（重复序号非安全形态）——即 v3.20.2-15 承诺的
1114 条投影游标已在用户的「备份、修复并重新打开」中修复完成。
