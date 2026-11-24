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

## 资源预取 {#prefetching}

`prefetch(None, None)` 在浏览器 `load` 事件后启用激进预取；传入 `Some(3)` 使用最多三个并发链接的瀑布策略，第二参数 `Some("vite:ready")` 可改为自定义 window 事件。

`use_waterfall_prefetching(None)` 沿用之前的并发数，初始值为 3；`use_aggressive_prefetching()` 改为一次插入全部链接。`use_prefetch_strategy(None, None)` 关闭预取，受支持的策略名称是 `Some("waterfall")` 和 `Some("aggressive")`；未知名称在渲染时返回错误。开发热更新模式不输出预取脚本。

生产清单中的动态导入及其依赖和 CSS 按原顺序发现，按 URL 去重并排除已经预加载的资源。脚本在指定事件后执行；瀑布策略在链接加载成功或失败后各释放一个名额，激进策略一次插入全部链接。现有预加载属性回调也会用于预取，脚本使用当前 CSP nonce。

启用预取时，预加载属性回调返回 `None` 会产生 `DisabledPrefetchAttributes`，对应固定源码无法展开禁用值的错误边界。请返回受支持的属性映射，或明确关闭预取策略，不把渲染失败静默当成成功。事件名和标签属性来自受信任的应用配置。

```rust
Vite::prefetch(Some(3), Some("vite:ready"))?;
Vite::use_prefetch_strategy(None, None)?;
```

## 自定义宏 {#macros}

通过 `Vite::r#macro` 注册具类型回调，接收实际 `&Vite` 服务及拥有所有权的参数，使用 `macro_call::<返回类型, _>` 调用。参数和结果不经过 JSON，可以使用非 `Clone` 的 Rust 值；缺失名称、参数或结果类型不符均明确报错。

同步宏的参数和返回值可使用 `Rc<Cell<_>>` 等非 `Send` 值；持久回调仍须为 `Send + Sync`，参数和结果保留在本次调用中。

```rust
use openrust::support::{HtmlString, facades::Vite};

Vite::r#macro("scripts", |vite, entry: String| {
    vite.render([entry], None)
})?;
let tags: HtmlString = Vite::macro_call("scripts", "resources/js/app.js".to_owned())?;
```

`mixin` 安装具名 `ViteMacro::new` 回调集合；第二参数为 `true` 时替换同名宏，`false` 时保留。`mixin_using` 接收惰性工厂，保留同名宏时不执行工厂，工厂失败保留前面已成功注册的宏。`has_macro` 区分大小写；`flush_macros` 清理注册，普通 Vite `flush` 不会清理宏。

宏注册属于应用，跨其 HTTP／Artisan／Worker 作用域保留；调用时使用当前执行配置，包括本次请求或任务的 nonce。不同应用的注册表隔离。回调及被替换捕获值均在注册锁外执行或销毁，优先使用传入的服务，避免捕获强服务引用形成引用环。

从 Blade 调用自定义宏时，将参数注册为 `Vec<ViewValue>`，返回 `ViewValue`。名称在渲染时解析，支持编译后再注册；普通输出转义字符串，受信任标记使用已有原始输出语法。其他原生签名不会被静默转换成模板签名。原生 mixin 列表显式公开回调，不反射 Rust 私有方法。

```rust
use openrust::view::ViewValue;

Vite::r#macro("greeting", |_, arguments: Vec<ViewValue>| {
    let name = arguments.first().and_then(ViewValue::as_str).unwrap_or("OpenRust");
    Ok(ViewValue::String(format!("你好，{name}")))
})?;
```

```blade
{{ Vite::greeting('OpenRust') }}
```

## 支持边界 {#support-status}

Vite 的 32 个声明服务入口及实际调用路径已有验收和明确的原生适配；共享 Facade 的 mock／spy 契约仍在核对，不代表所有门面已闭合。

## 运行范围 {#scope}

这些渲染调用不会自行启动或守护 Node 开发服务器。
