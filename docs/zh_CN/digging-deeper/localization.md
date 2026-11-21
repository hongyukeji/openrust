# 本地化

## 配置语言

通过 `APP_LOCALE` 设置应用默认语言，通过 `APP_FALLBACK_LOCALE` 设置分组翻译的回退语言。使用 `Lang::withLocale` 为一次请求或任务操作指定语言，操作结束后恢复之前的语言。

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

表结构沿用 Spatie Laravel Translation Loader：每个组与键保存一行，`text` 是语言到字符串的 JSON 对象，例如 `{"en":"Hello :name","fr":"Bonjour :name"}`。语言键使用规范标识（`en_US`、`zh_CN`）。缺失或为 null 的语言保留文件值；翻译值必须为字符串，空字符串也是有效覆盖。组 `*` 保存字面键并保留点号，其他组展开 `nested.title`、`items.0` 等点分键。包命名空间与 Spatie 一样仅使用文件来源。重复键、父子键冲突、非法 JSON、键中空段及超过 128 层的键会使加载失败。

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
        if locales.first().is_some_and(|locale| locale == "fr_CA") {
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

应用配置、语言文件和文档路径统一使用下划线标识。语言小写，地区大写，文字代码首字母大写。例如 `en`、`en_GB`、`fr_CA`、`es_419`、`sr_Latn_RS`、`zh_CN`、`zh_TW`。输入接受连字符、下划线和不同大小写。`zh-Hans` 解析为 `zh_CN`，`zh-Hant` 解析为 `zh_TW`。显式指定的 `zh_HK`、`zh_Hant_HK` 等继续读取各自的语言文件。HTML 使用 `zh-CN`、`zh-TW` 等语言标签。已有语言文件需改为归一后的名称。

## 覆盖包语言文件 {#overriding-package-language-files}

要覆盖 `shop::cart.summary.heading` 等包翻译，在 `lang/vendor/shop/en/cart.json` 中定义同一个键。目录和文件名沿用原翻译的命名空间、语言和分组。应用只覆盖自己定义的条目，其余条目仍由包提供。

## Artisan 翻译 {#artisan-translations}

Artisan 使用 `APP_LOCALE` 显示命令列表标题、帮助和参数错误。内置英文、简体中文和繁体中文。应用可在 `lang/<locale>/console.json` 覆盖文本。翻译先查当前语言，再查 `APP_FALLBACK_LOCALE`，最后使用内置文本。没有翻译的命令描述沿用注册时的文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh_CN ./artisan list make
APP_LOCALE=zh_Hant ./artisan help list
```

可在 `console` 分组内使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等键调整帮助。参见 [Artisan 输出格式](./artisan.md#command-help-and-output-formats)。

生产环境确认提示和取消消息使用同一翻译分组。可在 `console.json` 中覆盖 `confirm.production` 或 `confirm.cancelled`。输入 `y` 或 `yes` 确认，直接回车取消。

## 契约语言设置

Translator 契约、具体实现和 Lang Facade 均提供 `getLocale`、`setLocale`、`getFallback`、`setFallback`。启动时设置应用默认值；请求／队列作用域内设置只影响当前执行。`withLocale` 支持嵌套并在离开后恢复，回退语言独立设置。旧自定义 Loader／Translator 消费者应将契约中的 snake_case 调用迁移为上述原名。

## 语言设置的所有权

语言及回退语言的读取方法返回拥有所有权的 `String` 快照。设置方法接收 `&self`，返回 `TranslationResult<()>`，非法语言标识返回错误。语言标识会规范化，例如 `zh-CN` 转为 `zh_CN`。启动值作为默认值，请求或队列任务期间的修改只保留在当前容器作用域。`withLocale` 只临时覆盖当前任务，完成后恢复外层语言。
