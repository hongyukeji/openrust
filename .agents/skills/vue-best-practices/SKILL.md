---
name: vue-best-practices
description: 实现和审查 Vue Web 管理界面的组件、响应式数据、路由、共享状态与测试。适用于 Vue 页面开发；uni-app 页面以 uniapp-development 的平台规则为主，不自动套用浏览器路由或 DOM 能力。
license: MIT; see LICENSE
metadata:
  upstream: https://github.com/vuejs-ai/skills
  revision: 198f0cd8f7f9c0a3d9b566ccc16de6677519ee78
  path: skills/vue-best-practices
  related-paths: skills/vue-router-best-practices, skills/vue-pinia-best-practices, skills/vue-testing-best-practices
  adaptation: Curated OpenRust workflow with on-demand routing state and testing references
---

# Vue 应用开发

根据上游四个 Vue 专项技能合并改写；按需加载细节，不强制每次读取整套参考资料。许可见 [LICENSE](LICENSE)。

## 沿用现有工程

以 [可选快速开发模块](../../project/plans/optional-rapid-development-modules.md)、应用 manifest、lockfile 和现有组件库为准。新 Web 组件优先 Composition API、TypeScript 与 `script setup`，不为局部修复批量转换旧组件。具体版本、包管理器与命令从实际工程读取。

Admin 的菜单、布局、表单、表格与主题先复用选定后台骨架，不再建同义封装。接口错误、分页、权限和认证适配 OpenRust 公共契约，不让模板 mock 接口反向决定后端模型。

## 组件与响应式

- 状态有明确所有者；派生值用纯 `computed`，副作用放生命周期或 watcher。`ref`/`reactive` 按更新方式选择，`shallowRef` 用于需要浅层追踪的对象，不强制替换所有基本类型 ref。
- Props 输入、typed emits 输出；`v-model` 只表达真实双向契约。不修改 props，不用不受信内容直接渲染 HTML。
- 按共同职责拆组件和 composable，不以行数硬拆。局部表单状态保持局部；纯计算用普通函数。
- 异步筛选、切换详情和退出登录需处理过期响应；组件、缓存页面与订阅各有清理边界。按锁定 Vue 版本选择 cleanup API。
- 列表用稳定身份作为 key；行编辑、选择与排序不能因索引 key 串状态。

## 按需参考

- 导航、菜单、角色与共享状态：[路由和状态](references/routing-state.md)。
- 组件、composable 与端到端回归：[Vue 测试](references/testing.md)。
- 界面设计：[impeccable](../impeccable/SKILL.md)；只在 UI 任务涉及时读取。

执行实际工程的类型检查、lint、组件测试和构建；用户流程与 Rust API 联调按 [webapp-testing](../webapp-testing/SKILL.md) 验收。声明哪些测试使用 mock，不用截图或类型检查代替行为结果。
