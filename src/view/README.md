# `openrust::view`

`ViewServiceProvider` 显式安装后在启动时加载并校验模板目录。

启用 `view` Cargo feature 并注册 `openrust::view::ViewServiceProvider`，即可使用服务端 HTML 模板。应用模板放在项目根 `views/`，静态 CSS、图片等仍放在 `public/`。

```toml
[server]
root_target = "view:index.html"

[view]
directory = "views"
auto_reload = false
```

在本地 `.env` 中设置 `APP_VIEW_AUTO_RELOAD=true` 可启用开发期模板热重载；部署环境默认关闭。

`openrust new` 已默认生成这组配置和 `views/index.html`。部署时可把 `root_target` 改为 `file:/index.html`、`forward:/path` 或 `redirect:/path`；设置 `view:` 时必须启用 feature、注册 `ViewServiceProvider` 并提供模板，否则 router 构建失败。

在应用处理器中注入 `openrust::view::Views` 并调用 `render`：

```rust,ignore
#[derive(serde::Serialize)]
struct HomeContext { title: String }

async fn home(
    openrust::http::Inject(views): openrust::http::Inject<openrust::view::Views>,
) -> openrust::Result<openrust::http::axum::response::Html<String>> {
    views.render("pages/home.html", &HomeContext { title: "Welcome".into() })
}
```

模板只接受以 `.html` 结尾的相对名称，例如 `pages/home.html`，支持 `MiniJinja` 的继承、`include`、`macro` 与 filters。HTML 默认转义，缺少 context 字段会报错；符号链接也不能越出配置目录。只向模板传入已授权的数据，不把用户上传模板交给 renderer。渲染同步运行且每次执行有 fuel 上限；模板数据准备应在 handler 中完成，避免在模板中做 I/O 或业务查询。

模块 Provider 可在 `boot` 对同一个 `Views` 实例调用 `add_namespace("blog", source_dir, published_dir)`。随后用 `blog::pages/home.html` 渲染，`published_dir` 中同名模板优先，缺失时读取模块源目录。模板继承与 include 也应使用命名空间名称。重复命名空间在启动时失败；禁用模块不调用登记，因此不会贡献模板。`source_dir` 必须是已存在目录，模块内路径先经 `ModuleConfigFiles::resource` 校验，避免路径穿越和符号链接。

根 `view:` 首页自动提供读取运行时 `app.name` 的 `app_name`。其他页面由业务路由显式提供 context，并按需调用 `Views::render` 或请求感知入口，返回正常 HTML 响应，不套用 JSON API 包装。MiniJinja 的编译期邮件模板仍由独立 `mail-templates` feature 管理。

需要 Laravel 风格的请求错误包时，处理器同时提取 `RequestInfo`，调用 `views.render_for_request("pages/form.html", &info, &context)`。该入口要求 context 是 map 或 struct；显式 `errors` 键（包括 `null`）优先于请求共享包，非 map 输入报 `view_context_invalid`。`render` 仍供无需请求共享值的模板使用；`server.root_target = "view:..."` 使用同一个请求感知入口。

安装 Session Provider 后，Session middleware 在启动会话之后、进入处理器之前恢复 `errors`，并建立本请求私有的共享 `ViewErrorBag`。处理器可用 `RequestInfo::put_view_error_bag(name, Box<dyn MessageBagContract>)` 添加命名包；同一请求后续渲染立即可见。如果错误包已存在于 Session，该对象的修改同步到 Session，随后按原有保存与 flash 老化规则处理，不因普通 `put` 延长 flash；`Session::flash_errors` 显式闪存错误，已有 flash 也可通过 `keep`/`reflash` 延续。Session 整包替换 `errors` 后，当前视图仍持有原对象，原对象后续修改不影响替换值。缺失错误包时创建的空包不会自动写回 Session。模板可调用 `errors.count()`、`errors.any()`、`errors.all()`、`errors.first("email")`、`errors.getBag("login").first("email")`、`.get("email")`、`.count()` 和 `.toArray()`；消息包方法分派到恢复后的 `MessageBagContract`，包括已登记 codec 的自定义包；根 `errors.any()` 按 `ViewErrorBag` 语义使用默认包的 `count() > 0`，命名包的 `any()` 仍分派其自定义实现。没有 Session Provider 时每请求得到独立空包，缺失首条消息为 `""`。恢复错误会拒绝请求，不会改为静默空包。错误文本仍由 HTML 自动转义，应用不得在共享 `Views` 或模板环境保存请求数据。

错误包方法与 codec 在状态锁之外运行；重新进入同一对象的修改或递归恢复会报错，避免死锁。命名包、完整请求对象和持久记录均先验证候选预算，失败保留原值。`Session::read_errors()` 返回独立读取快照；请求内共享修改使用 `RequestInfo::put_view_error_bag`。

模板查询参数遵循已固定的消息包语义：`has`/`missing` 首参必需，`hasAny()` 等价于空键列表；首参为列表或关联字典时使用其值并忽略后续位置参数，标量首参使用全部位置参数。`none` 选择任意消息；空包的 `has([])` 为 false，非空包为 true，`hasAny([])` 为 false，`missing([])` 为 true。键列表复用 Collection 的单次 1024 字段预算，超限拒绝；字典读取值而非别名键。列表内元素和标量键接受字符串或整数，不模拟 PHP 任意对象、布尔或浮点的弱类型转换。

`first()` 与 `first(none, none)` 都读取首条消息，`get` 的 key 仍不可省略；`get(none)` 按固定 PHP8.5 的实际行为查询空字符串键，不模拟 PHP 弃用警告。`first/get/all/unique` 的 format 省略或显式 `none` 都使用消息包当前格式；`unique` 在格式化后去重，保留原键。`format()`/`getFormat()` 读取当前格式，不接受设置参数。无参数 getter 保持明确的 Rust 方法参数边界，不模拟 PHP 对无关多余参数的静默容忍。
