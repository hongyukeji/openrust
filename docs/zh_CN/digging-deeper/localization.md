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

## Artisan 翻译 {#artisan-translations}

Artisan 使用 `APP_LOCALE` 显示命令列表标题、帮助和参数错误。内置英文、简体中文和繁体中文。应用可在 `lang/<locale>/console.json` 覆盖文本。翻译先查当前语言，再查 `APP_FALLBACK_LOCALE`，最后使用内置文本。没有翻译的命令描述沿用注册时的文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh_CN ./artisan list make
APP_LOCALE=zh_Hant ./artisan help list
```

可在 `console` 分组内使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等键调整帮助。参见 [Artisan 输出格式](./artisan.md#command-help-and-output-formats)。

生产环境确认提示和取消消息使用同一翻译分组。可在 `console.json` 中覆盖 `confirm.production` 或 `confirm.cancelled`。输入 `y` 或 `yes` 确认，直接回车取消。
