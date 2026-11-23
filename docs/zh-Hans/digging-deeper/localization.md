# 本地化

## 配置语言

通过 `config/app.rs` 的 `locale`、`fallback_locale` 配置默认语言及缺失分组词条的回退语言，对应根 `.env`：

```dotenv
APP_LOCALE=en
APP_FALLBACK_LOCALE=en
```

按 Laravel 教程，使用 `App::setLocale` 切换一次 HTTP 请求的语言，使用 `App::currentLocale` 读取语言，使用 `App::isLocale("en")?` 判断语言。路由示例：

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response, StatusCode, abort},
    routing::RoutingError,
    support::facades::{App, Route},
};

pub fn register() -> Result<(), RoutingError> {
    Route::get("/greeting/{locale}", greeting)
}

fn greeting(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    let locale = request.route("locale").unwrap_or("en").to_owned();
    Box::pin(async move {
        if !["en", "zh-Hans", "zh-Hant"].contains(&locale.as_str()) {
            return Err(abort(StatusCode::BAD_REQUEST, "", Default::default()));
        }
        App::setLocale(&locale).await.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        let current = App::currentLocale().map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Response::from(current))
    })
}
```

`App::getLocale` 是 `currentLocale` 的同义入口；`getFallbackLocale`、`setFallbackLocale` 读写回退语言。`setLocale` 同步更新 `app.locale` 和译器，并发送 `foundation::events::LocaleUpdated`；事件包含 `locale` 和 `previousLocale`。Rust 的事件监听器可以异步执行，因此此方法需要 `.await`，失败通过 `Result` 返回。请求和队列任务内的修改互相隔离；启动阶段设置则作为该应用的默认值。

服务错误在实际调用步骤传播：先写配置，再更新译器，最后分发事件。常驻译器首次解析时分别读取应用默认值和当前作用域配置，避免把请求语言保存为应用默认值。监听器失败不会回滚本次已更新的配置和译器，错误保留原始来源；当前生命周期结束后仍按作用域恢复。重复设置相同语言也会分发事件，修改回退语言不分发该事件。

配置、API、资源、文档 URL、HTML 和 HTTP 统一使用 BCP 47。`setLocale("zh-CN")` 后仍为 `zh-CN`；`isLocale("ZH-cn")` 为真，`isLocale("zh-Hans")` 为假。规范化保留文字体系、地区、变体和支持的扩展，资源选择单独处理。旧下划线输入会明确报错。

`App` Facade 当前提供上述语言方法；其他 Laravel App 方法未通过这个 Facade 暴露。模板继续使用下文的翻译助手。`Lang::withLocale` 仅用于临时译器语言作用域，不修改应用语言配置。

### 浏览器语言 {#welcome-page-language}

OpenRust 默认自动装配 `openrust::http::middleware::SetLocale`，应用无需创建语言中间件或在 `bootstrap/app.rs` 手动注册。

默认候选在 HTTP 启动时从应用翻译资源发现一次；随附资源提供 `en`、`zh-Hans`、`zh-Hant`。中间件为配置的协商头复用请求语言解析器，按权重选择、跳过 `q=0`，同权重保持浏览器顺序。没有匹配项时使用受支持的 `APP_LOCALE`，否则使用按标签稳定排序后的首个候选。控制器、校验和 Blade 模板共用当前请求语言。

`zh-Hans`、`zh-CN`、`zh-SG` 和仅 `zh` 对应简体；`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO` 对应繁体，明确的文字体系优先于地区，未知文字体系不按中文地区猜测。数字变体不作为文字体系。其他语言匹配完整标识或基础语言。此选择规则不改变公开语言标签的规范化语义。

通过 [`app.supported_locales`](#available-locales) 配置即可限制语言、指定顺序或启用非文件来源。需要在代码中覆盖配置时，在 `bootstrap/app.rs` 的 `withMiddleware` 中显式指定候选：

```rust
.withMiddleware(|middleware| {
    middleware.setLocale(&["en", "fr", "de"]).expect("语言标签有效");
})
```

候选按 BCP 47 规范化；非法标签返回 `TranslationError` 并保留之前的设置，重复调用使用最后一次成功设置。传入空列表 `middleware.setLocale(&[])` 关闭识别，此时不修改语言或响应头，也不触发语言更新事件，应用继续使用 `app.locale` 和手动 `App::setLocale`。仅用于特定路由时，先关闭全局识别，再通过已有路由或路由组中间件注册 `SetLocale::new(&["en", "fr"])?`。

中间件在执行下游前调用 `App::setLocale`，同步配置、译器和 `LocaleUpdated` 事件；监听器失败时交由现有异常处理，不继续执行下游。下游处理完成（包括已渲染的异常响应）后读取最终语言，在缺少时添加 `Content-Language`，将启用的语言请求头（默认 `Accept-Language`）合并到 `Vary`，保留已有值或通配符。自动选择不保存偏好，不改变其他请求、队列任务或应用默认语言。默认路由装配位于输入转换与安全 Cookie 建立之后，详见[自动与固定语言](#locale-preference)；之前已返回响应的 CORS 预检、维护模式或请求体大小检查不会经过语言选择。

原先应用中的 `SetLocale` 文件和 `append` 注册应移除，避免重复执行；需要关闭识别时改用上述空列表设置。

欢迎页的 HTML `lang` 与 `Content-Language` 使用应用当前语言，显示文案按配置进行翻译回退。仓库地址来自框架包元信息，模板使用应用当前语言拼接文档路径。默认的 `en`、`zh-Hans`、`zh-Hant` 均有对应文档目录。关闭识别并设置 `APP_LOCALE=fr` 时，文案可以回退到英文，但文档链接仍指向仓库未提供的 `docs/fr`。使用其他应用语言时，请提供对应文档或定制欢迎页模板。

### 可用语言 {#available-locales}

普通应用可直接通过 `.env` 配置 `app.supported_locales`，无需编写中间件回调：

```dotenv
APP_SUPPORTED_LOCALES=en,zh-Hans,zh-Hant,ja
```

环境变量未设置或为 `null`（也支持 `(null)`，大小写不限）时自动发现资源。`APP_SUPPORTED_LOCALES=` 生成空列表，关闭语言选择和欢迎页菜单；逗号分隔的列表按指定顺序启用语言，也支持仅来自数据库的语言。标签规范化并按首次出现去重；非法标签、列表中的空项或错误配置类型在启动时报错。直接配置接受 JSON 数组或 `null`，不接受逗号分隔字符串。

候选优先级为显式 `middleware.setLocale(...)` → `app.supported_locales` → 资源自动发现，显式空列表同样优先。配置列表和显式列表均跳过扫描。默认中间件在 HTTP 启动时保存一次候选；修改后重启应用，使用配置缓存时同时重建缓存。本配置不改变 `app.locale`、`app.fallback_locale` 或请求来源优先级。


`FileLoader::availableLocales()` 发现 `<语言>.json` 和包含分组 JSON 资源的语言目录，也支持嵌套分组。扫描路径与翻译加载一致：存在 `resources/lang` 时使用该目录，否则使用 `lang`，并包含 `addPath` 登记的应用路径和 `addJsonPath` 登记的顶层 JSON 文件。包命名空间、`vendor`、隐藏项、符号链接和空语言目录不会启用语言。文件及目录名必须已是规范 BCP 47 标签，例如使用 `zh-Hans`，不用 `zh_CN` 或 `ZH-hans`。结果去重并按标签排序；发现过程不解析词条内容，也不判断翻译是否完整。

默认中间件在应用 Provider 启动完成后、接收请求前保存这份候选。每个请求不再扫描；添加或移除语言资源后需重启应用。候选发现独立于配置缓存。显式 `setLocale` 优先于配置；未显式设置时，非 null 的 `app.supported_locales` 列表跳过扫描。未发现语言时保留应用配置的语言并隐藏欢迎页切换菜单。仅来自数据库的语言通过显式候选启用；发现过程不查询数据库，也不自动启用所有包的语言。

例如增加 `lang/ja/welcome.json`，以及内容为 `{"name":"日本語"}` 的 `lang/ja/languages.json`，即可增加日语，无需修改控制器或菜单。缺少文案继续使用正常翻译回退。模板可通过 `Lang::hasForLocale('languages.name', $choice)` 判断该语言自身是否提供名称。右至左页面在该语言的 `languages.json` 中增加 `"direction":"rtl"`，欢迎页据此设置 HTML `dir="rtl"`；菜单支持长列表滚动。未提供方向时使用文档默认的左至右布局。新增界面翻译不会创建文档目录，欢迎页文档链接仍须对应实际提供的文档。

### URL 语言选择 {#locale-query}

`?locale=en`、`?locale=zh-Hans`、`?locale=zh-Hant` 只覆盖本次请求，不保存或替换固定偏好。顺序为回调 → URL → 受保护的偏好 Cookie → 配置请求头 → 默认语言或首个候选。回调返回 `None` 才进入下一来源。显式 URL 值规范化后必须精确属于候选；非法、空值、数组或不支持的值返回 HTTP 400。重复标量键沿用查询解析器的后值覆盖规则。请求体字段不参与本次语言选择。

在 `config/app.rs` 的语言区域配置：

```rust
"locale_query_parameter": get(env, "APP_LOCALE_QUERY_PARAMETER", "locale")?,
```

设置 `APP_LOCALE_QUERY_PARAMETER=language` 后只读 `?language=...`。缺键默认 `locale`，JSON `null` 只关闭 URL 选择。参数名须为非空顶层键，不含空白、控制字符、点或方括号，错误配置类型明确报错。环境值仍是字符串。修改后重建配置缓存；请求使用已加载的仓储，不重新读取 `.env`。

查询参数和 Cookie 默认名称现统一为 `locale`（`?locale=zh-Hans`，Cookie 加密前为 `locale=zh-Hans`）。默认不再读取旧 `lang` Cookie，请重新选择语言并更新旧书签。需保留旧名称时，显式设置 `APP_LOCALE_COOKIE=lang` 和／或 `APP_LOCALE_QUERY_PARAMETER=lang`。升级后重建配置缓存；缓存中显式保存的名称仍会生效。保存入口保持 `POST /locale`，表单字段默认为 `locale`。

### 自动跟随与固定语言 {#locale-preference}

从未手动设置时，Web 页面每次请求重新协商语言。欢迎页菜单遍历中间件的同一份有效候选，并提供“跟随浏览器”。受现有 Web CSRF 中间件保护的 POST 表单保存选择，“跟随浏览器”删除偏好；自动识别和 GET 不保存偏好。下划线表示本次实际语言，勾选表示固定选择或自动模式，URL 临时覆盖时也分别显示。母语名称来自各语言的 `languages.json`；该语言缺少名称时显示语言代码，不回退为其他语言的名称。POST 后以 303 返回表单指定的站内页面，保留其他查询参数并移除临时语言参数；关闭 URL 识别不影响保存表单。

OpenRust 扩展 `app.locale_cookie` 默认 `"locale"`，缺键相同，应用保留 `APP_LOCALE_COOKIE` 覆盖名称。JSON `null` 关闭偏好读取与写入（环境字符串 `null` 是名称）。名称须为非空顶层 Cookie 名称，不能与 Session 或 `XSRF-TOKEN` 冲突，非法配置报错。必须经过 `EncryptCookies`，不能豁免加密。默认实例自动装配在 Web Cookie 解密和队列之后、普通 Web Session 之前。无状态 API 不读取原始偏好 Cookie，也不自动启动 Session。没有安全 Cookie 队列时，欢迎页在 URL 识别启用时保留单次 GET 链接。

偏好复用 `Cookie::forever`（400 天）、加密、HttpOnly，以及现有 CookieJar 的 `session.path`、`domain`、`secure`、`same_site` 默认值（通常为 `/`、当前主机、HTTPS 时 Secure、Lax）。删除使用相同路径与域名。仅提供 HTTPS 的部署应设 `session.secure=true`。过期、清除 Cookie 或更换浏览器都会丢失本地选择，不承诺永久或跨设备同步。损坏、非法或已不属于候选的已保存值被忽略并恢复自动；显式非法提交返回 HTTP 400。过期由客户端执行，重放的 Cookie 请求头本身不携带过期信息。

受保护的 Web 写入入口可使用以下 OpenRust 扩展，只读取指定顶层请求体字段，由控制器决定响应：

欢迎页表单字段在 `config/app.rs` 中独立配置：

```rust
"locale_form_field": get(env, "APP_LOCALE_FORM_FIELD", "locale")?,
```

设置 `APP_LOCALE_FORM_FIELD=preferred_locale` 可同时修改欢迎页表单与保存入口。模板使用 `config('app.locale_form_field')`，保存入口严格使用 `Config::string("app.locale_form_field")`，均不另设兜底；默认值 `locale` 由应用配置定义。缺键、JSON `null` 或非字符串会使保存入口报错且不写入偏好。请使用非空顶层字段名，避免与 `redirect`、`_token`、`_method` 冲突。修改后重建配置缓存。此配置不修改 Cookie 或 URL 参数名；通用保存方法仍接受调用方指定的字段。

```rust
let redirect = match request.post(Some("redirect")).await? {
    None => "/".to_owned(),
    Some(serde_json::Value::String(path)) => path,
    _ => return Err(HttpError::InvalidRequest),
};
let target = SetLocale::urlWithoutLocale(request, &redirect)?;
SetLocale::storePreferenceFromRequest(request, "locale").await?;
Ok(Redirect::to(&target, RedirectOptions {
    status: StatusCode::SEE_OTHER,
    ..Default::default()
})?.into())
```

欢迎页表单通过请求体字段 `redirect` 提交当前页面路径。其他页面可传 `/account?tab=profile`，保存后返回该页面；缺失、null 或空值兜底为 `/`，错误类型和非站内目标在写入偏好前返回400。查询参数不能补充此字段。这是应用表单约定，不是新增中间件配置。

偏好字段缺失，或其值为数字、布尔、数组、对象时返回 HTTP 400；JSON `null` 或空字符串清除偏好，非空字符串复用现有 BCP 47 和候选校验。查询参数不会补齐请求体字段。字段取自既有输入中间件处理后的请求体；默认裁剪和空字符串转换可能将纯空白转为清除操作。此方法不改变本次请求语言，也不绕过请求方法、Cookie 管线或 CSRF 约束。已有 `storePreference(request, Some("zh-Hans"))` 和 `storePreference(request, None)` 仍可处理经过校验的值。

`urlWithoutLocale(request, "/")` 返回应用指定的目标，不会再次跳到保存入口。它从目标和当前请求中移除实际语言查询键，保留其他值及编码，使用现有 URL 生成器处理可信代理子路径；请求中的其他值覆盖目标中的同名值。只接受以单个 `/` 开头的本地路径，拒绝外部地址、协议相对地址和反斜杠。它只生成 URL，不保存偏好或创建响应。页面示例需导入 `SetLocale`、`Redirect`、`RedirectOptions` 和 `StatusCode`。

使用同一受保护 Cookie 管线的 API 控制器也可以读取自定义 `language` 字段，直接返回 204 或自行生成 JSON：

```rust
SetLocale::storePreferenceFromRequest(request, "language").await?;
Ok(Response::empty(StatusCode::NO_CONTENT))
```

这不会自动为普通无状态 API 启用持久化；须明确接入既有有状态 Cookie 管线和写入保护，浏览器请求仍需 CSRF。辅助方法不会注册路由、启动 Session 或增加账号存储。两个语言偏好辅助入口均属于 OpenRust 扩展。

每次写请求选择其中一种操作，再跳转至应用控制的站内地址。方法拒绝读取请求及不可用或未加密的偏好存储，只排队 Cookie，不改变本次语言或再次发事件，不能代替 CSRF 中间件。`LocaleSelection` 提供 `preference`、`can_store`、候选和 URL 信息；实际语言使用 `App::currentLocale()`。回调已决定语言时，不显示无法覆盖它的控件；空候选关闭整个中间件。

安全偏好页面通过现有响应头合并机制设置 `Cache-Control: private, no-store` 和 `Vary: Cookie`，未保存偏好时也适用。请求头识别启用时另合并实际头名，保留已有 `Vary` 和 `*`；路由缓存指令不能将这些变体公开缓存。账号等私有来源的回调仍须设置适当的应用缓存策略，尤其是未接入 Cookie 管线时。

主题菜单提供浅色、深色和跟随系统。自动模式响应系统变化，手动主题存入 localStorage（`appearance`），跟随系统会清除覆盖。禁用 JavaScript 后语言表单仍可使用，主题跟随系统。

### API 客户端偏好 {#api-locale-preference}

独立前端和移动端自行保存固定选择。自动模式每次发送当前系统／浏览器语言，固定模式通过配置请求头（默认 `Accept-Language`）发送保存的标签，恢复自动时清除客户端固定选择。发送语言头不会创建服务端永久偏好、Cookie、Session 或账号记录，只有接入翻译资源的文案变化。仓库提供服务端能力及消费者行为，没有实现移动端存储。

同源有状态浏览器 API 只有在管线于 SetLocale 之前完成 Cookie 解密和队列装配后，才可复用相同偏好。内置有状态 API 包装器为可信前端来源提供此管线，普通 API 仍无状态；接入后同样按回调 → URL → Cookie → 请求头排序，写入口保留 CSRF 保护。返回 JSON 本身不会启用偏好。

需要跨设备同步时，在应用账号设置中保存语言，并在认证之后的实例 `localeUsing` 回调中返回已校验的可信候选；账号未设置则返回 `None`，继续 Cookie 与浏览器兜底。框架不要求或创建用户模型字段和账号表。关闭默认实例、将同一个中间件放到依赖之后的示例见[自定义语言选择](#custom-locale-selection)。

### 语言偏好请求头 {#locale-header}

API 客户端默认传入标准请求头：

```http
Accept-Language: zh-Hans
```

在 `config/app.rs` 中，`"locale_header": get(env, "APP_LOCALE_HEADER", "Accept-Language")?` 指定应用语言协商请求头。设置 `APP_LOCALE_HEADER=X-Language` 后只读取该头，头名按 HTTP 规则不区分大小写；该头缺失时不再尝试 `Accept-Language`。值继续使用同一 BCP 47 校验、权重、多头合并和候选匹配规则。缺键默认 `Accept-Language`；JSON `null` 仅关闭请求头识别。环境值按字符串读取：`APP_LOCALE_HEADER=null` 表示名为 `null` 的请求头，并不关闭识别；关闭时请在应用配置中设置 JSON `null`。空名称、非法名称或非字符串类型属于配置错误（HTTP 500），即使回调或 URL 已选择语言也会报告。空候选仍关闭整个中间件。

选择顺序为 `localeUsing` 返回 `Some` → 配置的 URL 参数 → 受保护的偏好 Cookie → 配置的请求头 → 应用默认语言及候选兜底；回调返回 `None` 才进入下一来源。配置使用已有仓储和配置缓存，修改源码或环境不会覆盖已加载的缓存，修改 `APP_LOCALE_HEADER` 后需重建配置缓存。可配置请求头和默认 URL 识别属于 OpenRust 扩展。

`Request::getLanguages`／`getPreferredLanguage` 始终描述标准 `Accept-Language`，不受应用配置影响。只有接入翻译资源的文案随语言变化。启用请求头识别时，`Vary` 合并实际配置的头名，保留已有项和 `*`；`null` 不添加语言头的项，仍保留 URL、回调和 `Content-Language`。依赖 Cookie 或用户资料的回调仍需应用设置适当的私有缓存策略及其他 `Vary` 项。

业务、控制器和页面读取实际使用的语言时使用 `App::currentLocale()?`；URL、持久化偏好回调或下游 `App::setLocale` 都可能让它不同于请求偏好。例如配置 `APP_LOCALE_HEADER=X-Language`，同时收到 `Accept-Language: en` 和 `X-Language: zh-Hant` 时，Request 仍返回英文偏好，App 与翻译使用繁体中文。停用 SetLocale 后，配置本身不会自动执行语言选择。

### 自定义语言选择 {#custom-locale-selection}

普通自定义协商头直接设置既有 `APP_LOCALE_HEADER=X-Locale`，不设置时协商 `Accept-Language`。该头位于 URL 和受保护 Cookie 偏好之后；请求缺少配置的自定义头时，使用默认语言或首个候选，不再尝试 `Accept-Language`。这与优先于其他来源的回调不同。常规候选列表使用上方配置，用户或租户语言等业务规则再使用回调。

`localeUsing` 是 OpenRust 扩展接口，可为当前应用的中间件实例设置同步回调。例如优先读取指定请求头，缺少时依次尝试 URL、受保护的偏好 Cookie 与请求头协商：

```rust
.withMiddleware(|middleware| {
    middleware.localeUsing(|request| {
        Ok(request.header("X-Locale")?.map(str::to_owned))
    });
})
```

回调返回 `Result<Option<String>, HttpError>`：

- `Ok(Some(locale))` 先规范化 BCP 47 标签，再与规范化后的候选列表精确匹配，不执行浏览器别名、基础语言匹配或翻译回退。默认候选下，`ZH-hANT` 选择 `zh-Hant`；`zh-CN`、`en-US`、空字符串和非法标签均报错。若需选择地区或扩展标签，请将有效标签明确加入候选，选中后保留完整标识。
- `Ok(None)` 先尝试配置的 URL 参数和受保护的偏好 Cookie，再使用原有浏览器协商、受支持的应用默认语言或候选首项。
- `Err(error)` 将原错误交给现有异常处理并停止下游。非法或不受支持的选择作为内部错误处理；若值来自用户输入，可在回调内校验并返回适当的 HTTP 错误。选择失败不切换语言，也不触发 `LocaleUpdated`。

未设置回调时行为不变。候选与回调独立设置，调用顺序不限；候选更新失败保留原候选及回调，重复设置回调使用最后一个。空候选关闭整个实例，不执行回调、不触发事件、不处理响应头，但保留回调以供重新启用。路由实例也提供 `setLocale` 与 `localeUsing`，候选更新与回调只属于该实例，不使用进程全局注册表；回调自身也应避免共享可变用户状态。

默认实例在解析后的路由管线执行，位于直接 Cookie 解密／队列之后、普通 Web Session 和认证之前。有状态 API 包装器中则在其 Cookie／Session 保护之后执行。没有这些依赖的路由保持无状态；未匹配错误页和公开静态响应仍执行请求头／URL 选择，不启用 Web 存储。回调可以读取请求头及此时已准备的信息，不能访问后续才提供的依赖。依赖 Session 时，先在 `withMiddleware` 调用 `middleware.setLocale(&[]).expect("语言标签有效")` 关闭全局识别，再在 `routes/web.rs` 中注册同一个中间件，放到默认 `web` 组加载 Session 之后：

```rust
use std::sync::Arc;
use openrust::{
    contracts::session::Session,
    http::{Response, middleware::SetLocale},
    routing::{RouteGroup, RoutingError},
    support::facades::Route,
};

pub fn register() -> Result<(), RoutingError> {
    let mut locale = SetLocale::new(&["en", "zh-Hans", "zh-Hant"])
        .expect("语言标签有效");
    locale.localeUsing(|request| {
        Ok(request.session()?.get("locale")
            .and_then(|value| value.as_str().map(str::to_owned)))
    });
    Route::group(RouteGroup::new().middleware(Arc::new(locale)), |router| {
        router.get("/account", || Response::from("Account"))?;
        Ok(())
    })
}
```

示例要求已配置 Session 驱动和应用加密密钥；只读取已有的 `locale` Session 值，不保存新偏好。没有值时依次尝试 URL、受保护的偏好 Cookie 与请求头协商。依赖认证的规则同样应放到认证之后；影响本地化路由绑定的规则应在绑定替换之前执行，需要这种位置时通过现有 `Middleware::group`／`MiddlewareEntry` 显式配置组内顺序。增加回调不会自动调整执行顺序。异步查询应由前置中间件完成，再通过请求传递已准备的数据；此回调本身同步执行。

这些来源统一由 `SetLocale` 调用 `App::setLocale`、派发事件并处理响应头。下游主动切换语言后，缺少的 `Content-Language` 使用最终语言，已有值保持不变。由于回调可能回退浏览器协商，仍会将启用的语言请求头（默认 `Accept-Language`）合并到 `Vary`。这**不能**完整隔离由 `X-Locale`、Cookie、Session 或用户资料选择的响应。请求头示例还需让响应按 `X-Locale` 区分；私有内容应设置 `Cache-Control: private, no-store` 或应用所需的缓存策略，并按需增加其他 `Vary` 字段。中间件保留这些响应头，不推断缓存键、不持久化回调结果，也不保证任意个性化响应可以安全进入共享缓存。安全 Cookie 偏好页面还会获得 `private, no-store` 与 `Vary: Cookie`。

### 在 Blade 中翻译 {#blade-translations}

模板可以直接读取翻译，控制器只需提供页面数据：

```blade
<h1>{{ __('welcome.heading') }}</h1>
<p>{{ trans('messages.greeting', ['name' => $name]) }}</p>
<p>{{ trans_choice('messages.items', $count) }}</p>
```

`__` 和 `trans` 接受字符串键、可选替换数组和可选语言；`trans_choice` 在键后增加数字或数组计数。省略语言或传入 `null`／空字符串时使用当前请求语言，显式指定语言只影响该次调用。替换值支持字符串、数字、布尔值和 `null`。缺键和回退语言遵循 `Lang` 的规则。

<code v-pre>{{ ... }}</code> 转义 HTML。`@lang('messages.title')`、`@choice('messages.items', $count)` 和 `{!! __('messages.title') !!}` 输出原始文本，仅用于可信 HTML 和替换值。子模板和组件继承渲染语言，编译缓存中的模板在每次渲染时读取翻译。翻译助手需要有效的应用翻译服务。翻译助手可以返回文本、分组或列表。分组及列表可用于 `@foreach`，或使用数组下标读取词条；直接输出整个数组会报错。Rust 调用方可使用 `Lang::array`。

`@lang ... @endlang` 支持翻译区块：先渲染并去除首尾空白，再把内容作为翻译键。`@lang(['name' => $name]) ... @endlang` 可传替换数组，输出原始文本。嵌套区块共享替换参数，内层开始时会替换外层的替换数组。`__()` 和 `__(null)` 返回 `null`，直接输出时为空。`@lang('messages.title', [], null, false)` 可关闭该次分组查找的回退；复数替换中的 `count => null` 使用实际计数，显式零值或空字符串仍保留。`trans()`／`trans(null)` 应返回译器对象，当前模板值模型未暴露服务对象，请在 Rust 中使用 `Lang`；任意 PHP 函数、Facade 方法调用和命名参数尚未实现。参数数量及类型显式校验，不依赖 PHP 的隐式类型转换。

## 发布语言文件 {#publishing-the-language-files}

运行 `./artisan lang:publish` 发布认证、分页、密码重置和验证的英文 JSON 语言文件。默认写入 `lang/en`，应用已有 `resources/lang` 时写入 `resources/lang/en`。默认保留现有文件。用 `--force` 覆盖四份文件，用 `--existing` 只覆盖已发布的文件。两个选项同时使用时，范围由 `--existing` 决定。

## 定义翻译字符串

分组翻译放在 `lang/en/messages.json` 等文件中，嵌套条目使用点号访问。以完整句子为键的翻译放在 `lang/en.json`。应用存在 `resources/lang` 时，使用该目录作为语言目录。

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

### 登记目录和运行期翻译行 {#registering-translation-sources}

在应用作用域内首次查找之前，用 `Lang::addPath`、`Lang::addJsonPath` 或 `Lang::addNamespace` 登记目录。较晚登记的目录覆盖较早的目录。分组资源递归合并，包括列表索引。JSON 字面键按整项覆盖。应用目录在 JSON 专用目录之后加载。

用 `Lang::addLines` 登记带点号的分组键。首次登记的分组以提供的翻译行为准。要扩展文件中的分组，先调用 `Lang::load`。加载后的分组在应用内缓存，后续路径登记作用于之后加载的分组。

```rust
use std::path::PathBuf;

use openrust::{
    support::facades::Lang,
    translation::{Line, TranslationResult},
};

pub fn register_translations(directory: PathBuf) -> TranslationResult<()> {
    Lang::addPath(directory)?;
    Lang::load(None, "messages", "en")?;
    Lang::addLines(
        [("messages.runtime".into(), Line::Text("Registered :name".into()))],
        "en",
        None,
    )
}
```

### 从自定义来源加载分组 {#loading-custom-translation-sources}

在应用的异步入口向 `Lang::loadFrom` 传入 `Loader`。每个资源指定语言、分组和可选命名空间。框架一次性替换这批分组。读取失败或其中一个分组在加载期间被运行期更新时，保留当前目录。之后的翻译查询仍为同步调用。

```rust
use openrust::{
    contracts::translation::Loader,
    support::facades::Lang,
    translation::{TranslationResource, TranslationResult},
};

pub async fn load_notice(source: &dyn Loader) -> TranslationResult<()> {
    Lang::loadFrom(
        source,
        &[TranslationResource {
            locale: "en",
            group: "notices",
            namespace: None,
        }],
    )
    .await
}
```

### 数据库翻译源（扩展） {#database-translation-loader}

在应用 Cargo 依赖中通过 `path = "vendor/openrust/translation-database"` 添加 `openrust-translation-database`。`DatabaseLoader` 接收显式的框架 `Connection`，可从当前应用的数据库管理器或 `DB::connection` 获取。支持 PostgreSQL 18、MySQL 8.4 和 SQLite 的数据库词条加载，其他数据库目标须分别验证行为。

先通过应用迁移创建表，再加载翻译。可使用以下最小结构：

```rust
use openrust::database::{Connection, DatabaseResult};

pub async fn create_translations(connection: &Connection) -> DatabaseResult<()> {
    connection.getSchemaBuilder().create("language_lines", |table| {
        table.id();
        table.string("group", None).index();
        table.string("key", None);
        table.json("text");
        table.timestamps();
    }).await
}
```

表结构沿用 Spatie Laravel Translation Loader：每个组与键保存一行，`text` 是语言到字符串的 JSON 对象，例如 `{"en":"Hello :name","fr":"Bonjour :name"}`。语言键使用规范标识（`en-US`、`zh-Hans`）。缺失或为 null 的语言保留文件值；翻译值必须为字符串，空字符串也是有效覆盖。组 `*` 保存字面键并保留点号，其他组展开 `nested.title`、`items.0` 等点分键。包命名空间与 Spatie 一样仅使用文件来源。重复键、父子键冲突、非法 JSON、键中空段及超过 128 层的键会使加载失败。

在异步应用作用域中调用以下函数，例如迁移就绪后由 Provider 的 `boot` 加载：

```rust
use std::{path::PathBuf, sync::Arc};
use openrust::{
    database::Connection,
    support::facades::Lang,
    translation::{FileLoader, TranslationResource, TranslationResult},
};
use openrust_translation_database::DatabaseLoader;

pub async fn load_database_translations(
    connection: Connection,
    language_directory: PathBuf,
) -> TranslationResult<()> {
    let source = DatabaseLoader::new(connection)
        .withTable("language_lines")?
        .withMaxRows(10_000)?
        .withFallback(Arc::new(FileLoader::new(language_directory)));
    Lang::loadFrom(
        &source,
        &[
            TranslationResource { locale: "fr", group: "messages", namespace: None },
            TranslationResource { locale: "en", group: "messages", namespace: None },
        ],
    ).await
}
```

表名采用所选连接的表前缀，只接受不带 schema 的标识符。默认表名为 `language_lines`，每个组默认最多读取 10,000 行；超过配置上限会报错，不截断结果。

`withFallback` 从文件来源加载相同语言、组和命名空间，再用数据库词条覆盖。组和列表索引递归合并，字面 JSON 值整体替换。数据库空字符串仍为有效覆盖；删除一行后，显式重新加载该资源即可恢复文件值。语言回退继续使用译器配置；需要数据库回退词条时，同时加载所选语言和回退语言资源。

`Lang::loadFrom` 一次发布全部请求组。数据库、文件或解析错误会返回失败，并保留先前发布的目录；没有词条的资源提供空数据库覆盖。数据库按组读取，发布原子性不提供横跨所有读取的数据库事务快照。重新加载由应用显式触发。写入、迁移和刷新时机由应用管理，包不会自动选择来源或启动后台任务。

与 Spatie 的 PHP 模型不同，本来源读取每行时不执行语言回退；OpenRust 译器在加载精确语言后按自身配置回退，也因此保留所选语言的文件词条。缺表会返回错误，避免显式重载失败时静默用文件替换已发布的数据库目录。

## 获取翻译字符串

使用 `__` 读取文本。占位符支持 `:name`、`:Name` 和 `:NAME`。分组和列表使用 `Lang::array` 读取。缺失翻译返回键本身。JSON 字面键在所选语言内查找，分组键可以回退到配置的语言。

### 语言回退与缺失键 {#translation-lookup-hooks}

使用 `Lang::has(key, locale, true)` 检查包含回退语言的翻译，使用 `Lang::hasForLocale(key, locale)` 仅检查指定语言。两种检查都会跳过缺键回调。在应用作用域内登记钩子后，可调整分组翻译的回退顺序，JSON 字面键仍优先。缺键回调返回的文本会继续处理占位符，返回 `None` 时使用原键。

```rust
use openrust::{support::facades::Lang, translation::TranslationResult};

pub fn register_lookup_hooks() -> TranslationResult<()> {
    Lang::determineLocalesUsing(|locales| {
        let mut locales = locales.to_vec();
        if locales.first().is_some_and(|locale| locale == "fr-CA") {
            locales.insert(1, "fr".into());
        }
        locales
    })?;

    Lang::handleMissingKeysUsing(|missing| {
        (missing.key == "messages.welcome").then(|| "Hello, :name".into())
    })
}
```

### 替换翻译字符串中的参数 {#replacing-parameters-in-translation-strings}

使用 `Lang::getWith` 按顺序传入文本、标签和对象。回调接收同一行上每个匹配的 `<name>…</name>` 的内容。标签回调按参数顺序执行，随后替换文本占位符。使用 `Lang::arrayWith` 替换分组或列表中的值。使用 `Lang::choiceWith` 时先选择复数分支，再替换。

#### 格式化对象替换值 {#object-replacement-formatting}

在应用作用域内用 `Lang::stringable` 为具体 Rust 类型注册格式化器，通常放在 `AppServiceProvider::boot`。类型必须注册格式化器时用 `Replacement::object`。允许在未注册时调用该类型的 `Display` 实现，则用 `Replacement::display`。Rust 枚举可显式传入变体名或值，也可为枚举注册格式化器。

例如，在 `lang/en/payments.json` 中定义 `payments.notice`。

```json
{"notice": "<mark>Paid</mark> :amount"}
```

```rust
use openrust::{
    support::facades::Lang,
    translation::{Replacement, TranslationResult},
};

pub struct Amount(pub u32);

pub fn register_formatters() -> TranslationResult<()> {
    Lang::stringable(|amount: &Amount| format!("{} USD", amount.0))
}

pub fn payment_notice(amount: &Amount) -> TranslationResult<String> {
    Lang::getWith(
        "payments.notice",
        &[
            ("amount", Replacement::object(amount)),
            ("mark", Replacement::tag(|text| format!("[{text}]"))),
        ],
        None,
    )
}
```

## 复数

使用 `trans_choice` 选择形式。先匹配 `{0}`、`[2,*]` 等显式条件，其余情况按命中翻译的地区复数规则选择。函数自动提供 `:count`。

```rust
use std::collections::BTreeMap;

use openrust::{
    support::facades::Lang,
    translation::{TranslationResult, __, trans_choice},
};

pub async fn summary(locale: &str) -> TranslationResult<(String, String)> {
    Lang::withLocale(locale, async {
        let replacements = BTreeMap::from([("name".into(), "ada".into())]);
        Ok((
            __("messages.welcome", &replacements)?,
            trans_choice("messages.items", 2_i64, &replacements)?,
        ))
    })
    .await?
}
```

语言标签使用连字符，例如 `en`、`en-GB`、`fr-CA`、`es-419`、`sr-Latn-RS`、`zh-Hans`、`zh-Hant`、`zh-Hant-HK`、`en-US-u-nu-latn` 和 `en-x-demo`。复用 ICU4X 2.4 解析器和 CLDR 别名数据规范大小写及弃用别名（如 `iw-IL` → `he-IL`），不自动补充文字或地区。这验证库支持的 Unicode Locale 语法，并非逐项校验 IANA 登记。grandfathered `i-klingon`、独立 private-use `x-demo` 及 extlang 等库不支持形式显式报错，支持的扩展不会静默丢弃。翻译按完整规范标签精确查找资源，分组缺键再使用配置回退，句子键 JSON 保留原有查找规则；不隐式把地区归并到文字目录。Number 消费 Unicode 扩展，Faker 只在私有 provider 边界转换数据键，不改变公开语言。

## 覆盖包语言文件 {#overriding-package-language-files}

要覆盖 `shop::cart.summary.heading` 等包翻译，在 `lang/vendor/shop/en/cart.json` 中定义同一个键。目录和文件名沿用原翻译的命名空间、语言和分组。应用只覆盖自己定义的条目，其余条目仍由包提供。

## Artisan 翻译 {#artisan-translations}

Artisan 使用 `APP_LOCALE` 显示命令列表标题、帮助和参数错误。内置英文、简体中文和繁体中文。应用可在 `lang/<locale>/console.json` 覆盖文本。翻译先查当前语言，再查 `APP_FALLBACK_LOCALE`，最后使用内置文本。没有翻译的命令描述沿用注册时的文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh-Hans ./artisan list make
APP_LOCALE=zh-Hant ./artisan help list
```

可在 `console` 分组内使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等键调整帮助。参见 [Artisan 输出格式](./artisan.md#command-help-and-output-formats)。

生产环境确认提示和取消消息使用同一翻译分组。可在 `console.json` 中覆盖 `confirm.production` 或 `confirm.cancelled`。输入 `y` 或 `yes` 确认，直接回车取消。

## 契约语言设置

Translator 契约、具体实现和 Lang Facade 均提供 `getLocale`、`setLocale`、`getFallback`、`setFallback`。启动时设置应用默认值；请求／队列作用域内设置只影响当前执行。`withLocale` 支持嵌套并在离开后恢复，回退语言独立设置。旧自定义 Loader／Translator 消费者应将契约中的 snake_case 调用迁移为上述原名。

## 语言设置的所有权

语言及回退语言的读取方法返回拥有所有权的 `String` 快照。设置方法接收 `&self`，返回 `TranslationResult<()>`，非法标签返回错误；`zh-CN` 和 `zh-Hans` 保持不同。启动值作为默认值，请求或队列任务期间的修改只保留在当前容器作用域。`withLocale` 只临时覆盖当前任务，完成后恢复外层语言。

## 升级语言标识 {#bcp47-upgrade}

先暂停接收新任务并排空旧邮件／通知队列，再停止旧 Web 和队列进程；无法排空时保留任务供专用工具迁移。变更资源或持久化语言值前，备份配置、语言资源及受影响数据，避免新旧版本混跑。显式更新 `APP_LOCALE`、`APP_FALLBACK_LOCALE`、`APP_FAKER_LOCALE`，应用不会自动修改实际 `.env`。

| 旧项目值／路径 | 新值／路径 |
| --- | --- |
| `zh_CN`、`lang/zh_CN`、`docs/zh_CN` | `zh-Hans`、`lang/zh-Hans`、`docs/zh-Hans` |
| `zh_TW`、`lang/zh_TW`、`docs/zh_TW` | `zh-Hant`、`lang/zh-Hant`、`docs/zh-Hant` |
| 地区 `en_US`、`en_GB`、`zh_HK`、`zh_Hant_HK` | `en-US`、`en-GB`、`zh-HK`、`zh-Hant-HK` |

前两项仅用于本项目默认资源及已确认归属的数据；合法的 `zh-CN`、`zh-TW` 仍为独立地区标签。迁移目录时，同步句子键 JSON 文件和包覆盖资源；先检查目标文件及 JSON 键冲突，冲突时保留两份内容供人工处理。默认不安装旧 URL 重定向或下划线输入别名，需要过渡时由托管层显式配置。

替换旧二进制前，使用旧版本执行 `./main artisan config:clear`、`./main artisan view:clear`，源码方式可用既有 `./artisan` 入口。新代码、资源和配置同时部署，再执行 `config:cache`、`view:cache` 重建缓存并重启所有常驻进程；进程内翻译目录缓存随重启清除。旧邮件／通知队列应先排空；无法排空时，由应用专用工具检查序列化 `locale` 字段，加密任务还需要旧应用密钥。不要在队列、共享缓存或设置 JSON 中做通用文本替换。

数据库翻译先只读盘点 `language_lines.text` 的 JSON 语言键，以及应用自有设置或语言列。`vendor/openrust/translation-database/database/upgrade-bcp47/`提供独立只读盘点及 PostgreSQL、MySQL、SQLite 事务，映射默认两种中文及 `en_US`／`en_GB`；审阅后按实际情况调整表名和其他旧语言映射。未列入映射的下划线键、任意新旧键碰撞（包括相同值或 null）均使事务失败，不覆盖内容。默认末尾为 **ROLLBACK**，核对结果和备份后，才显式改为 **COMMIT**。发生错误需回滚并关闭该专属数据库会话。成功后重复执行为空操作；已提交后的回退需要恢复备份，不能凭新键名判断哪些内容来自迁移。

Faker 原生 provider 键仅保留在私有边界，`locale()`、`randomLocale()` 输出 BCP 47；仅有文字体系的 `zh-Hans`／`zh-Hant` 使用已有对应中文 provider，公开标签仍保留原地区与扩展。微信原生 `lang=zh_CN` 等第三方协议字段保持不变。


## 替换复数选择器

`Translator::getSelector()` 返回当前 `Arc<dyn Selector>`。实现 `openrust::translation::Selector` 后传入 `setSelector`，即可自定义复数分支选择。其 `choose` 接收原始文案、`PluralCount` 和解析后的语言，返回 `TranslationResult<String>`，之后才替换 `:count` 等占位符。通过 `setSelector(MessageSelector)` 恢复默认选择器。`getLoader()` 返回实际共享的 `Arc<FileLoader>`；先前取出的选择器和加载器句柄仍然有效。

## 语言目录

应用创建时，若 `resources/lang/` 已存在，OpenRust 优先使用它，否则选择根目录的 `lang/`。读取路径不会创建目录。应用 Contract 提供 `langPath("")`，也可传入 `langPath("en/validation.json")` 等后缀。

需要自定义目录时，在应用装配阶段、解析翻译加载器之前设置：

```rust
use openrust::contracts::foundation::Application as _;

app.useLangPath(app.base_path().join("translations"))?;
```

在 HTTP、Artisan 或队列作用域内，`App::langPath("")` 与 `openrust::support::lang_path("")` 读取同一应用路径，也可使用 `App::useLangPath(...)`。目录属于整个应用，不应逐请求切换。已经解析的加载器保留原目录，因此应在加载服务之前配置，不依赖热切换。

`lang:publish` 发布到此目录，默认保留已有文件；`--force` 覆盖可发布模板，`--existing` 只覆盖已经发布的模板。包资源可将 `app.langPath("vendor/package/...")` 传给 `publishesPaths` 作为目标；`vendor:publish` 支持明确配置的绝对路径，包括项目外目录，仍拒绝相对路径穿越及符号链接。包翻译的应用覆盖也从此语言目录加载。
