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
