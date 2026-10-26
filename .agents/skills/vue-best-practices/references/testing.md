# Vue 测试边界

改编自同一上游快照的 `vue-testing-best-practices`，来源与许可见 [技能入口](../SKILL.md)。

- 使用工程已有 runner 和 Vue Test Utils 等已选工具；组件测试验证用户输入、渲染、事件和错误状态，避免只断言巨大 HTML 快照。
- `trigger`/`setValue` 等异步操作按 API 等待；`nextTick` 等 Vue DOM 更新，`flushPromises` 处理待执行 Promise 回调。二者都不保证真实网络或尚未触发的 timer 完成；时钟按测试 runner 显式推进。
- 有生命周期或 inject 的 composable 放在适当的挂载上下文；测试结束卸载并检查 timer、监听器和订阅清理。
- Pinia 测试明确是真实 action 还是 stub；需要验 action 副作用时不能使用默认 stub 后宣称已覆盖。
- 路由参数变化、搜索响应乱序、登录刷新失败与退出后的旧响应写回有回归用例。
- 弹层、焦点、布局与浏览器 API 不能仅靠无布局 DOM 环境证明。真实浏览器流程按 [webapp-testing](../../webapp-testing/SKILL.md) 执行。
- 保存视觉基线前审查差异；不能更新快照来掩盖行为回归。

参考 [Vue 官方测试指南](https://vuejs.org/guide/scaling-up/testing.html) 与 [Vue Test Utils 异步说明](https://test-utils.vuejs.org/guide/advanced/async-suspense.html)，以工程版本为准。
