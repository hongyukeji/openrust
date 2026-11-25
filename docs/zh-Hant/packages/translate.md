# 文字機器翻譯

在應用的 Cargo 預設功能中啟用 `translate`，並保留既有預設功能。套件提供 `Translate` 門面與非同步 `Translator` 契約，用於單筆純文字。應用文案繼續透過 `Lang` 查詢語言資源。

## 設定具名服務

在應用設定載入器中使用 `translate` 鍵。`default` 選擇具名服務，每個服務的 `driver` 選擇已註冊的工廠。可在產生設定時用 `TRANSLATE_DRIVER` 選擇預設服務。以下設定宣告一個測試用的標籤驅動程式。

```rust
let profiles = serde_json::json!({
    "default": "preview",
    "drivers": {
        "preview": {"driver":"example","label":"Preview:"}
    }
});
```

## 註冊自訂驅動程式

在應用或相依套件中實作 `Translator`，並將下例 Provider 加入應用 Provider 清單。範例驅動程式回傳附有標籤的原文，用來說明介面。

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

## 翻譯文字

省略 `with_source` 即可請求驅動程式辨識來源語言。語言使用 `en-US` 或 `zh-Hans` 等 BCP 47 標籤。請求保留非空白原文前後的空白。結果包含譯文，服務商若有回傳來源語言，結果也會提供該值。

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

每個應用按照設定名稱快取驅動程式。修改設定或替換工廠後，呼叫 `Translate::manager()?.forget_drivers()`，後續便會解析新實例。舊控制代碼繼續使用原設定。應用環境或設定載入器有變更時，請重建應用設定快取。
