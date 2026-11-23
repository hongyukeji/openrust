# 资源构建／Vite

OpenRust 通过 `openrust::support::facades::Vite` 解析 JavaScript／CSS 入口清单，生成开发或生产资源标签。底层服务为 `openrust::foundation::Vite`，无需额外安装命令；前端项目仍需自行安装和启动 Vite。原有 `@fonts` 字体配置见 [Blade 模板](blade.md)。

## Blade 入口 {#available-entry-points}

```blade
@vite(['resources/css/app.css', 'resources/js/app.js'])
@viteReactRefresh

<img src="{{ Vite::asset('resources/images/logo.png') }}">
<meta name="csp-nonce" content="{{ Vite::cspNonce() }}">
```

## 资源配置 {#configuration}

在服务提供者的 `boot` 中配置应用默认值。默认构建目录为 `public/build`，清单名为 `manifest.json`，开发标识文件为 `public/hot`；用 `use_hot_file(path)` 指定其他标识文件。`@vite` 接受一个入口字符串或入口数组，第二参数可指定构建目录。`with_entry_points`／`merge_entry_points` 配置 `to_html()` 使用的入口，普通 `@vite` 仍使用自己明确传入的入口。

```rust
use openrust::support::facades::Vite;

Vite::use_build_directory("build")?;
Vite::use_manifest_filename("manifest.json")?;
Vite::with_entry_points(["resources/js/app.js"])?;
Vite::use_script_tag_attributes([
    ("defer".into(), true.into()),
    ("data-app".into(), "OpenRust".into()),
].into())?;
```

## 生产与开发模式 {#asset-modes}

前端构建时启用 manifest，并将输出写入配置的公开构建目录。`Vite::render(entries, None)` 返回 `HtmlString`，`to_html` 返回字符串。框架递归解析静态导入及其 CSS，去重共享标签，将 CSS 标签排在入口脚本前，并依据实际清单生成预加载标签和完整性摘要。`preloaded_assets()` 返回预加载属性；`flush()` 清空本次执行状态，保留配置和应用内的清单缓存。

开发标识文件存在时，其内容提供开发服务器 URL，标签包含 `@vite/client` 与指定入口。`react_refresh()`／`@viteReactRefresh` 仅在开发模式生成 React 前置脚本。`asset(path, build)` 返回 URL，`content(path, build)` 返回原文件字节，即使开发模式也仍读取生产清单；构建目录参数用 `None` 采用默认值。`manifest_hash(None)` 在开发模式或清单缺失时返回 `None`，其余情况返回当前文件 MD5；摘要读取独立于已解析清单缓存。

## 回调与隔离 {#callbacks-and-lifecycle}

用 `create_asset_paths_using` 自定义资源 URL，`clear_asset_path_resolver` 恢复 URL 生成器。标签静态属性或 `use_script_tag_attributes_using`／`use_style_tag_attributes_using` 回调接收实际入口、URL、资源块与清单组成的 `TagContext`，开发模式后两者为空。`use_integrity_key(Some("integrity"))` 选择摘要字段，`None` 禁用摘要；`use_preload_tag_attributes(None)` 禁用预加载。预加载回调返回 `Option<Attributes>`，`None` 停止该标签。

回调执行时不持配置锁，可重入配置；会失败的属性回调使用 `*_using_fallible`，返回 `ViteResult`，可用 `ViteError::callback(error)` 保留实际错误。属性值支持标量文本／数字，true 表示独立属性，false／null 表示省略；这些值来自受信任的应用配置，嵌套属性返回 `InvalidAttribute`。标签在属性回调之后生成，回调修改 nonce 的生效时机与固定契约一致。

Provider 默认值持续保留。HTTP、Artisan 和每个 Worker 任务分别拥有执行配置、nonce 与预加载状态，一个生命周期的覆盖不会成为下一个生命周期的默认值。在 HTTP 中间件调用 `Vite::use_csp_nonce(None)?` 生成 40 位 nonce，再用返回值设置响应 CSP 头。路由持有可变请求、需要调用视图或响应门面时，沿用 `request.with_container(async { ... }).await` 借出请求。

## 字体 {#fonts}

`Vite::fonts(None)` 返回全部字体的 `HtmlString`；用 `Vite::fonts(Some(&["sans", "mono"]))?` 按顺序选择别名，`Some(&[])` 表示不选字体。`@fonts` 和 `Vite::fonts` 都委托同一服务，与普通 Vite 资源共用预加载去重记录。

生产字体清单默认为 `public/build/fonts-manifest.json`，在 Provider 中通过 `use_build_directory` 和 `use_fonts_manifest_filename` 修改。开发模式读取已配置 hot 标记旁的 `fonts-manifest.dev.json`，也支持公开目录之外的自定义标记；清单缺失时不输出标签。

版本 1 清单包含 `families`、`preloads` 和可选 `style`。筛选调用按别名顺序输出 `familyStyles` 与 `variables`；未筛选时优先 `style.inline`，再读取 `style.file`。未知别名或本次选中预加载条目无效会返回错误。预加载回调收到 `source = "fonts"`、空资源块和空清单，可以覆盖属性或禁用字体链接；链接与内联样式都使用当前 CSP nonce。

`flush()` 还会清空应用的已解析字体清单和预加载状态，保留配置及 JavaScript／CSS 清单缓存。缺失文件和 JSON 解析失败不缓存。字体 CSS 文件读取保留公开目录能力边界，并拒绝含样式结束标签的 CSS；清单属性与内联 CSS 应来自受信任的构建输出。

```rust
Vite::use_fonts_manifest_filename("fonts-manifest.json")?;
let fonts = Vite::fonts(Some(&["sans", "mono"]))?;
```

## 支持边界 {#support-status}

预取策略和具类型 Vite 宏仍在补齐，不应根据本页推断这些入口已经可用。

## 运行范围 {#scope}

这些渲染调用不会自行启动或守护 Node 开发服务器。
