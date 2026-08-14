# Codex Provider 菜单投影与路由入口收口设计

## 目标

- 普通 Codex Provider 表单不再显示旧的“Codex 多模型路由”编辑入口。
- 既有 `settingsConfig.codexRouting` 继续读取、迁移、保存和运行，MultiRouter 工作台继续作为唯一可见编辑入口。
- 新建非官方 Codex Provider 默认开启“在 Codex `/model` 菜单中显示”。
- 菜单投影开关保留在“高级选项”内；默认开启本身不能让高级区自动展开。

## 数据边界

不修改 `codexRouting` schema、旧数组迁移、Rust route resolver、Provider 切换接管或 MultiRouter 工作台。普通 Provider 表单仍向 `useCodexConfigState` 传递并保存原有路由值，只隐藏旧编辑控件。

`meta.codexLocalModelMapping` 继续作为单 Provider 模型目录投影开关。显式保存的 `false` 必须保留；只有新建非官方 Provider 的缺省值改为 `true`。官方 Provider 不写该字段，MultiRouter 启用 route 时仍由既有后端规则强制投影聚合目录。

## 界面行为

- 普通 Provider 表单不出现“Codex 多模型路由”“添加路由”等控件。
- “在 Codex `/model` 菜单中显示”只在高级选项展开后可见。
- 新建 Provider 打开表单时，高级选项保持折叠；展开后开关默认为开启。
- 编辑旧 Provider 时，显式 `false` 不被升级为 `true`。

## 验证

- 组件测试证明旧路由入口不可见。
- Provider 表单测试证明新建默认开启、显式关闭保持关闭。
- 既有 hook、MultiRouter、Rust 路由兼容测试保持通过。

