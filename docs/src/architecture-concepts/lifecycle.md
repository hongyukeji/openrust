# 请求生命周期与应用启动

默认 `src/main.rs` 只启动 `bootstrap/app.rs` 中的应用。应用通过 `ApplicationDefinition::config()` 提供代码配置清单，通过 Provider 清单和模块清单提供启动贡献。

启动先选择 dotenv 和部署环境，再求值配置函数，向同一 `ConfigRepository` 发布配置。框架、应用和模块 Provider 进入同一个 Container：`register` 登记服务，`boot` 使用已登记服务挂载路由和资源。模块不是第二套容器或独立子应用。

应用 Controller 放在 `app/http/controllers/`，通过 `routes/api.rs` 或 `routes/web.rs` 声明访问路径；应用 Provider 类型放在 `app/providers/`，由 `bootstrap/providers.rs` 登记。不要把这些应用职责放入框架 `src/`。

API 请求匹配当前配置前缀的无状态路由组，再进入处理函数。异常经公开 Handler 扩展渲染；根应用对配置的 API 范围选择 JSON 包装，Web 请求继续使用其内容协商与异常渲染流程。

Provider 顺序、模块加载和配置错误可阻止应用启动。修改模块持久状态只影响后续应用实例，不会热卸载当前 Provider 或路由；详见[模块状态](../modules/file-state.md)。

对应固定 Laravel 13.x 官方章节：[lifecycle](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/lifecycle.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。
