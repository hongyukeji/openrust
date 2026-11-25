# 文本机器翻译

在应用 Cargo 默认功能中启用 `translate`，并保留已有默认功能。该包提供 `Translate` 门面和异步 `Translator` 契约，用于单条纯文本。应用文案继续通过 `Lang` 查询语言资源。

## 配置具名服务

在应用配置加载器中使用 `translate` 键。`default` 选择具名服务，每个服务的 `driver` 选择已注册工厂。可在生成配置时用 `TRANSLATE_DRIVER` 选择默认服务。以下配置声明了一个测试用的标签驱动。

```rust
let profiles = serde_json::json!({
    "default": "preview",
    "drivers": {
        "preview": {"driver":"example","label":"Preview:"}
    }
});
```

## 注册自定义驱动

在应用或依赖中实现 `Translator`，并将下例 Provider 加入应用 Provider 列表。示例驱动回显带标签的原文，用来演示接口。

```rust
use std::sync::Arc;

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use openrust_translate::{
    TranslateManager, Translation, TranslationError, TranslationFuture,
    TranslationRequest, Translator,
};

struct Preview(String);

impl Translator for Preview {
    fn translate(&self, request: TranslationRequest) -> TranslationFuture<'_> {
        Box::pin(async move {
            Ok(Translation::new(format!("{} {}", self.0, request.text())))
        })
    }
}

struct AppTranslationProvider;

impl ServiceProvider for AppTranslationProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<TranslateManager>()?.extend("example", |profile| {
                let label = profile["label"]
                    .as_str()
                    .ok_or(TranslationError::InvalidConfiguration("label"))?;
                Ok(Arc::new(Preview(label.to_owned())))
            });
            Ok(())
        })
    }
}
```

## 翻译文本

省略 `with_source` 时请求驱动识别源语言。语言使用 `en-US` 或 `zh-Hans` 等 BCP 47 标签。请求保留非空白原文首尾的空白。结果包含译文，并在服务商返回源语言时提供该值。

```rust
use openrust_translate::{Translate, TranslationRequest, TranslationResult};

async fn preview() -> TranslationResult<String> {
    let request = TranslationRequest::new("你好", "en")?.with_source("zh-Hans")?;
    let default = Translate::translate("你好", "en").await?;
    let named = Translate::driver("preview")?.translate(request).await?;
    assert_eq!(default.text(), named.text());
    Ok(named.into_text())
}
```

每个应用按配置名缓存驱动。修改配置或替换工厂后，调用 `Translate::manager()?.forget_drivers()`，后续调用会解析新实例。旧句柄继续使用原配置。应用环境或配置加载器有变更时，重建应用配置缓存。

## 内置驱动

在具名配置中设置下表的 `driver`。`api_key_env` 保存环境变量名。启用配置缓存后，通过 HTTP、Artisan 及各 Worker 的进程环境注入密钥。每个服务默认超时 10 秒，可用 `timeout_ms` 设置。

| 驱动 | 配置 | 语言映射 |
| --- | --- | --- |
| `deepl` | `plan` 选择 `free`（默认）或 `pro`，同时设置 `api_key_env`。`base_url` 可覆盖官方 Free 或 Pro 地址，驱动追加 `/v2/translate`。 | 区域或文字体系明确的目标包括 `en-GB`、`en-US`、`pt-BR`、`pt-PT`、`zh-Hans` 和 `zh-Hant`。显式源语言使用基础语言。 |
| `libretranslate` | 设置自托管的 `base_url`，可选 `api_key_env`。驱动追加 `/translate`。 | `zh-Hant` 和 `pt-BR` 分别使用繁体中文 `zt`、巴西葡萄牙语 `pb` 模型。支持的语言对取决于服务端安装的模型。 |
| `tencent` | 设置 `api_key_env` 和 TokenHub 专用翻译模型，例如 `hy-mt3-plus`。`base_url` 可选择其他 TokenHub 站点。 | 调用 `/v1/api/translations` 并发送 `stream=false`。`zh-Hant` 映射为 `zh-TR`，自动识别时省略 source。 |
| `aliyun` | 设置 `access_key_id_env` 和 `access_key_secret_env`，临时凭据可用 `security_token_env`。`region` 默认 `cn-hangzhou`。 | 对纯文本表单做 ACS3 签名后调用 `TranslateGeneral`。`zh-Hant` 映射为 `zh-tw`，自动识别使用 auto。 |
| `baidu` | 配置通用文本服务的 `app_id_env` 和 `api_key_env`。 | 按原始 UTF-8 文本签名后才做表单编码。`zh-Hant`、ja、ko 对应 `cht`、`jp`、`kor`。译文分段按顺序用换行连接。 |

```json
{
    "default": "office",
    "drivers": {
        "office": {"driver": "deepl", "plan": "pro", "api_key_env": "DEEPL_API_KEY"},
        "local": {"driver": "libretranslate", "base_url": "http://127.0.0.1:5000"}
    }
}
```

远程端点使用 HTTPS，本机服务可使用回环 HTTP。每次翻译发送一次 POST，收到重定向时直接返回错误。超时时服务商可能已经处理该请求。
