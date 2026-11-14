# 加密

## 簡介

應用程式加密支援 AES-128 和 AES-256 的 CBC 與 GCM 模式，預設使用 AES-256-CBC。CBC 用 HMAC-SHA256 驗證承載資料，GCM 使用驗證標籤。

## 設定

在環境檔案中設定 `APP_KEY`。Artisan 依應用程式設定的密碼演算法產生金鑰，並取代所選環境檔案中既有且未加引號的 `APP_KEY=` 賦值。正式環境取代非空金鑰時需要確認或傳入 `--force`。`--show` 會輸出新金鑰供安全轉存，同時保留原檔。

```sh
./artisan key:generate
```

### 平滑輪替加密金鑰

將舊金鑰保留在以逗號分隔的 `APP_PREVIOUS_KEYS` 中。加密始終使用目前的金鑰，解密先嘗試目前的金鑰，再依設定順序嘗試舊金鑰。使用新設定重新啟動應用程式程序，並將舊金鑰保留到相關資料無須再解密為止。

```ini
APP_KEY=base64:<current-key>
APP_PREVIOUS_KEYS=base64:<previous-key>
```

## 使用加密器

在請求或命令作用域中使用 `Crypt` 門面。`decryptString` 傳回位元組，承載資料無效或驗證失敗時傳回錯誤。可用 `encrypt(value, true)` 和 `decrypt(payload, true)` 與 PHP 的序列化位元組字串互通。

```rust
use openrust::{
    encryption::{DecryptException, EncryptException},
    support::facades::Crypt,
};

pub fn encrypt_token(token: &str) -> Result<String, EncryptException> {
    Crypt::encryptString(token)
}

pub fn decrypt_token(payload: &str) -> Result<Vec<u8>, DecryptException> {
    Crypt::decryptString(payload)
}
```
