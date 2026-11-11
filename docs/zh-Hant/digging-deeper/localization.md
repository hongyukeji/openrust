# 在地化

## 設定語言

透過 `APP_LOCALE` 設定應用程式的預設語言，透過 `APP_FALLBACK_LOCALE` 設定群組翻譯的備援語言。使用 `Lang::withLocale` 為一次請求或工作操作指定語言，操作結束後恢復先前的語言。

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
