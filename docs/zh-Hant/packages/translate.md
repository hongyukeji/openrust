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

## 內建驅動程式

在具名設定中指定下表的 `driver`。`api_key_env` 保存環境變數名稱。啟用設定快取後，透過 HTTP、Artisan 及各 Worker 的程序環境提供金鑰。每個服務的預設逾時為 10 秒，可用 `timeout_ms` 設定。

| 驅動程式 | 設定 | 語言對應 |
| --- | --- | --- |
| `deepl` | `plan` 選擇 `free`（預設）或 `pro`，並設定 `api_key_env`。`base_url` 可覆寫官方 Free 或 Pro 位址，驅動程式接上 `/v2/translate`。 | 有明確地區或文字體系的目標包括 `en-GB`、`en-US`、`pt-BR`、`pt-PT`、`zh-Hans` 和 `zh-Hant`。明確指定來源語言時使用基礎語言。 |
| `libretranslate` | 提供自託管的 `base_url`，可選 `api_key_env`。驅動程式接上 `/translate`。 | `zh-Hant` 和 `pt-BR` 分別採用繁體中文 `zt`、巴西葡萄牙語 `pb` 模型。支援的語言組合取決於伺服器已安裝的模型。 |
| `tencent` | 設定 `api_key_env` 與 TokenHub 專用翻譯模型，例如 `hy-mt3-plus`。`base_url` 可選擇其他 TokenHub 站點。 | 呼叫 `/v1/api/translations` 並送出 `stream=false`。`zh-Hant` 對應 `zh-TR`，自動辨識時省略 source。 |
| `aliyun` | 設定 `access_key_id_env` 和 `access_key_secret_env`，臨時憑證可用 `security_token_env`。`region` 預設為 `cn-hangzhou`。 | 對純文字表單執行 ACS3 簽名後呼叫 `TranslateGeneral`。`zh-Hant` 對應 `zh-tw`，自動辨識使用 auto。 |
| `baidu` | 設定通用文字服務的 `app_id_env` 和 `api_key_env`。 | 使用原始 UTF-8 文字簽名後才編碼表單。`zh-Hant`、ja、ko 分別對應 `cht`、`jp`、`kor`。譯文分段依序以換行連接。 |
| `azure` | 用 `api_key_env` 指定訂閱金鑰。區域和多服務資源須設定 `region`。`base_url` 可包含自訂資源位址的路徑前綴。 | 呼叫 `/translate`，送出 `api-version=3.0` 和一個 `to`。自動辨識時省略 `from`。保留 `zh-Hans` 與 `zh-Hant` 的區別。 |
| `google` | 設定 `project` 和 `location`（預設 `global`），可選 `quota_project`、`base_url`。程序使用 Google 應用程式預設憑證。 | 呼叫 Cloud Translation Advanced v3，採用 `text/plain`。`zh-Hans`、`zh-Hant` 對應 `zh-CN`、`zh-TW`。自動辨識時省略 `sourceLanguageCode`。 |

透過反向代理存取 LibreTranslate 時，將路徑前綴寫入 `base_url`，例如 `https://translate.example.com/mt`。每個應用程式和 Worker 都應信任反向代理的 TLS 憑證。

```json
{
    "default": "office",
    "drivers": {
        "office": {"driver": "deepl", "plan": "pro", "api_key_env": "DEEPL_API_KEY"},
        "local": {"driver": "libretranslate", "base_url": "http://127.0.0.1:5000"}
    }
}
```

遠端端點使用 HTTPS，本機服務可使用回送 HTTP。每次翻譯只送出一次 POST，收到重新導向時立即傳回錯誤。逾時時服務商可能已處理該請求。

Google 驅動程式使用官方 Rust 驗證函式庫。提供受信任的 `GOOGLE_APPLICATION_CREDENTIALS` 檔案，或使用具備應用所需 Cloud Translation 權限的執行身分。憑證檔案與應用程式設定分開存放。驗證函式庫快取 OAuth 存取權杖並依需要更新。
