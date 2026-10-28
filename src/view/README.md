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

根首页自动提供 `app_name`。其他页面由业务路由调用 `Views::render`，返回正常 HTML 响应，不套用 JSON API 包装。MiniJinja 的编译期邮件模板仍由独立 `mail-templates` feature 管理。
