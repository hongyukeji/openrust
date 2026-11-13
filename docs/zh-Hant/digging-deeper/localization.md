# 在地化

## 設定語言

透過 `APP_LOCALE` 設定應用程式的預設語言，透過 `APP_FALLBACK_LOCALE` 設定群組翻譯的備援語言。使用 `Lang::withLocale` 為一次請求或工作操作指定語言，操作結束後恢復先前的語言。

## 發佈語言檔案 {#publishing-the-language-files}

執行 `./artisan lang:publish` 發佈驗證身分、分頁、密碼重設和欄位驗證的英文 JSON 語言檔案。預設寫入 `lang/en`，應用程式已有 `resources/lang` 時寫入 `resources/lang/en`。預設保留既有檔案。用 `--force` 覆寫四份檔案，用 `--existing` 僅覆寫已發佈的檔案。兩個選項同時使用時，由 `--existing` 決定範圍。

## 定義翻譯字串

群組翻譯放在 `lang/en/messages.json` 等檔案中，巢狀項目使用點號存取。以完整句子為鍵的翻譯放在 `lang/en.json`。應用程式存在 `resources/lang` 時，會使用該目錄作為語言目錄。

```json
{
  "welcome": "Hello, :Name",
  "items": "{0} No items|{1} One item|[2,*] :count items"
}
```

## 取得翻譯字串

使用 `__` 讀取文字。預留位置支援 `:name`、`:Name` 和 `:NAME`。群組和清單使用 `Lang::array` 讀取。缺少翻譯時回傳鍵本身。JSON 字面鍵在所選語言內查找，群組鍵可使用設定的備援語言。

## 複數

使用 `trans_choice` 選擇形式。先比對 `{0}`、`[2,*]` 等明確條件，其餘情況依找到翻譯的地區複數規則選擇。函式會自動提供 `:count`。

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

## Artisan 翻譯 {#artisan-translations}

Artisan 使用 `APP_LOCALE` 顯示命令清單標題、說明和引數錯誤。內建英文、簡體中文和繁體中文。應用程式可在 `lang/<locale>/console.json` 覆寫文字。翻譯先查目前語言，再查 `APP_FALLBACK_LOCALE`，最後使用內建文字。沒有翻譯的命令說明沿用註冊時的文字。

```sh
APP_LOCALE=en ./artisan help list
APP_LOCALE=zh_Hans ./artisan list make
APP_LOCALE=zh_Hant ./artisan help list
```

可在 `console` 群組中使用 `options`、`commands.list.description` 和 `commands.list.arguments.namespace` 等鍵調整說明。請參閱 [Artisan 輸出格式](./artisan.md#command-help-and-output-formats)。

正式環境確認提示和取消訊息使用同一翻譯群組。可在 `console.json` 中覆寫 `confirm.production` 或 `confirm.cancelled`。輸入 `y` 或 `yes` 確認，直接按 Enter 取消。
