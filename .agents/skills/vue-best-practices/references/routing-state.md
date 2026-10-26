# Vue Web 路由与状态

改编自同一上游快照的 `vue-router-best-practices` 与 `vue-pinia-best-practices`；来源与许可见 [技能入口](../SKILL.md)。仅在 Web 工程采用这些库时应用，不作为 uni-app 路由规则。

- Web 路由守卫处理首次访问、匿名访问、登录回跳与权限加载；避免登录页再次重定向自身，回跳地址限制在允许范围。
- 同一组件的路由参数变化不保证重新挂载；监听目标参数或使用适当的更新守卫，取消/忽略旧请求。
- 路由和按钮权限是交互控制，实际 API 必须校验授权。退出和角色变化时按契约清理动态菜单、路由、缓存与敏感 store。
- 查询、分页等可分享状态可进入 URL；身份凭据和敏感搜索值不放 URL。表单草稿和全局登录状态不混为一个 store。
- Pinia 在正确应用上下文中初始化。解构 state/getters 时保留响应性，例如使用 `storeToRefs`；actions 按库支持方式使用。
- 持久化只保存允许字段并定义过期和清理规则；不得默认将整个身份 store、token、权限快照写入浏览器持久存储。
- 并发请求触发的 refresh 应协调为明确的单次刷新流程，失败有终止条件；不要无限重放非幂等业务请求。

具体 API 以项目锁定版本的 [Vue Router 文档](https://router.vuejs.org/) 和 [Pinia 文档](https://pinia.vuejs.org/core-concepts/) 为准。
