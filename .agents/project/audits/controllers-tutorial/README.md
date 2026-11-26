# 控制器教程逐节对账

固定上游文档和框架版本见[锚点清单](section-map.json)。已有控制器、路由和响应验收保存在[04-02](../04-02/README.md)。清单中的待复核条目保留已有证据，逐节核对完成后更新状态。

## 89. 模型和表单请求生成

用 PHP 8.5.12 实际运行固定生成器，对照普通模型、模型和表单请求、嵌套 API、嵌套单例四种形态。夹具仅替换终端提示组件。嵌套模板仍使用普通请求参数，即使命令另外生成表单请求文件。

原生命令提供 `--model`、`--parent` 和 `--requests`，复用模型、表单请求生成器和模板发布。Rust 显式注册控制器动作、绑定模型参数并声明表单请求处理器。生成的动作包含 501 占位，留待应用填入业务实现。现有请求文件在控制器强制重建时保留。

实际 Artisan 发布并修改模板，再生成并编译控制器和请求类型。SQLite 上 9 次真实 HTTP 覆盖普通及嵌套模型、未命中 404、创建和更新验证的 422，以及有效请求到达生成动作的 501。另验 82 项回归、255 个三语 Artisan 示例、12 组控制器浏览器检查、格式和隔离仓严格 Clippy。详见[验收记录](accepted-model-generation.json)。

## 91. 资源模型回退和软删除

固定 PHP 8.5.12 直接执行 Router 与 ResourceRegistrar，核对 19 条普通、嵌套和 API 资源路由的回退与软删除选项。资源级 missing 只应用于成员动作。with_trashed 默认覆盖 show、edit、update，with_trashed_for 可以指定动作，传空列表仍采用默认值。

Rust 复用现有 SubstituteBindings、Route::missing 和 Route::with_trashed，资源描述符把设置应用到各个实际路由。与普通路由一样，显式类型注册负责绑定模型。资源成员命中缺失回调时，实际生成指向命名列表路由的 302。嵌套集合找不到父模型保持 404。软删除、显式动作列表及 GET／PUT／PATCH／DELETE 接入 SQLite 和 MySQL 8.4.13，实际 route:cache 后各 12 次 HTTP 通过。两个数据库均为临时夹具，MySQL 容器已删除。

三语正文的 6 个 Rust 示例直接提取、编译并接到实际 HTTP 路由，验证 9 次请求。5 个路由目标 12 项回归、独立 Artisan route:list 和 inspire、实际 /up 与 SIGTERM、三语构建和 12 组浏览器、格式及隔离仓严格 Clippy 通过。详见[验收记录](accepted-resource-model-binding.json)。

## 92. 批量注册资源

用固定 PHP Router 执行普通、API 和软删除三类批量入口。真实 Router 和实际 Artisan route:list 的 38 条方法、路径、名称与控制器动作全部对账。Rust 使用 ResourceRegistration 为不同的控制器类型保留类型化描述，在同一批次中按序注册。后续条目注册失败时保留此前成功条目，失败条目未插入残缺动作。

实际 TCP 逐条访问 38 条路由并确认进入各自控制器。SQLite 与 MySQL 8.4.13 分别运行 8 次批量软删除资源请求，核对默认 show、edit、update 可绑定且 destroy 返回 404。正式三语的 9 段示例直接提取到实际应用中运行，另通过 114 次 TCP 请求。站点构建、12 组浏览器、格式和隔离仓严格 Clippy 通过。见[验收记录](accepted-resource-batches.json)。

## 93. 资源动作中间件

固定 PHP Router 复验普通、API、单例和 API 单例的 11 条路径。Rust 直接使用资源描述符的 middleware、middleware_for、without_middleware 和 without_middleware_for。复用原有路由中间件解析器。逐动作重复配置覆盖该动作的旧值，晚设通用中间件追加到已配置动作，过滤路由中间件后仍执行全局中间件。

实际 HTTP 验证 11 个执行序列与固定注册器一致。实际 Artisan route:list 查看生效的中间件，独立二进制也通过列表和 inspire。三语正文直接提取并实际运行各 2 次请求。浏览器、路由和中间件生命周期回归、格式和隔离仓严格 Clippy 通过。见[验收记录](accepted-resource-middleware.json)。

## 94. 控制器声明式中间件

固定 PHP Router 执行 HasMiddleware 静态声明、动作过滤、组与路由合并、闭包和排除。Rust 在控制器分发器上指定动作名，资源描述符使用约定的动作名，两个公开入口共享同一过滤器。编译后保留实际中间件实例，与路由中间件交同一管线解析。只有全局中间件在路由排除范围外。

真实 TCP 核对普通控制器和资源动作共 6 条执行顺序，包含空 only、only 和 except 交集、同名中间件去重、内联闭包、路由级排除。三语正文的 6 个 Rust 片段套入正式公开入口，执行 3 次真实请求。实际 Artisan、56 项路由／请求及生命周期回归、三语站点、12 组浏览器、格式和隔离仓严格 Clippy 通过。其余控制器属性和授权另按对应锚点跟踪。见[验收记录](accepted-controller-middleware.json)。

## 95. 嵌套资源绑定作用域

固定 PHP 8.5.12 直接使用上游 Router 和 ImplicitRouteBinding，对照 10 个请求路径。Rust 将调用 scoped 与字段覆盖分开处理。空列表仍启用父子约束，未指定的子字段采用模型默认值，重复声明替换字段列表，单路由显式关闭继续生效。

旧实现实际允许错误父模型的请求返回 200。修复后 SQLite 和 MySQL 8.4.13 各 10 次固定对照通过，覆盖普通绑定、空选项、两级关系、替换字段、跨父子拒绝及退出作用域。三语正文的 6 个 Rust 片段直接提取，在真实 SQLite HTTP 中执行对应的 15 次请求。应用内实际 route:cache、独立 Artisan、浏览器、格式和隔离仓严格 Clippy 通过。见[验收记录](accepted-nested-scopes.json)。

教程的关系名推断仍待独立接通。目前使用已有的类型化关系回调，测试明确声明父子关系。

## 96. 资源动作选取、命名和补充路由

直接运行固定 PHP Router 得到 23 条路由以及 5 条实际匹配。原生对照发现重复 names 和 parameters 保留旧映射，先在未修复的实际 Router 上复现，再使整组设置覆盖旧值。单独的 name 和 parameter 保留其他条目。动作选择、点号嵌套、浅层路径与成员匹配继续复用注册器。

Rust Router 与实际 Artisan route:list 的 23 条方法、路径、名称和动作一致。5 次真实 HTTP 验证补充路由登记先后、父子参数名和浅层成员。三语正文 18 个片段直接提取成路由并发出 18 次真实 HTTP，9 项外围回归、独立 Artisan、三语站点、12 组浏览器、格式和隔离仓严格 Clippy 通过。见[验收记录](accepted-resource-route-options.json)。
