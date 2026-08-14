# Codex Provider 菜单投影与路由入口收口设计

## 目标

- 普通 Codex Provider 表单不再显示旧的“Codex 多模型路由”编辑入口。
- 既有 `settingsConfig.codexRouting` 继续读取、迁移、保存和运行，MultiRouter 工作台继续作为唯一可见编辑入口。
- CCSwitchMulti 维护的内置 Codex 预设始终开启模型菜单投影，不向用户显示可关闭开关。
- 自定义 Codex Provider 默认开启模型菜单投影，并在“高级选项”最末保留关闭能力；默认开启本身不能让高级区自动展开。

## 数据边界

不修改 `codexRouting` schema、旧数组迁移、Rust route resolver、Provider 切换接管或 MultiRouter 工作台。普通 Provider 表单仍向 `useCodexConfigState` 传递并保存原有路由值，只隐藏旧编辑控件。

`meta.codexLocalModelMapping` 继续作为单 Provider 模型目录投影开关。对于带稳定 `codexPresetId` 的 CCSwitchMulti 内置预设，保存值强制为 `true`，已有误关值在下次保存时纠正；自定义 Provider 继续尊重显式 `false`。官方 Provider 不写该字段，MultiRouter 启用 route 时仍由既有后端规则强制投影聚合目录。

## 界面行为

- 普通 Provider 表单不出现“Codex 多模型路由”“添加路由”等控件。
- 内置预设不显示“在 Codex `/model` 菜单中显示”开关，并始终投射由 CCSwitchMulti 维护的正确模型目录。
- 自定义 Provider 的开关只在高级选项展开后可见，且位于全部高级控件的最后。
- 新建自定义 Provider 打开表单时，高级选项保持折叠；展开后开关默认为开启。
- 开关说明必须明确：它控制 Codex 启动时加载的模型目录，以及 `/model` 中的模型、别名、上下文窗口和推理档位；它不控制 Provider、代理或 MultiRouter 是否可用。关闭只适用于用户自行维护 `model_catalog_json` 的高级场景。
- 编辑旧自定义 Provider 时，显式 `false` 不被升级为 `true`。

## 验证

- 组件测试证明旧路由入口不可见。
- Provider 表单测试证明新建默认开启、显式关闭保持关闭。
- Provider 表单测试证明内置预设不暴露开关语义并在保存时强制开启，自定义来源仍允许关闭。
- 组件测试证明开关位于高级区最后且帮助文案完整覆盖影响范围。
- 既有 hook、MultiRouter、Rust 路由兼容测试保持通过。
