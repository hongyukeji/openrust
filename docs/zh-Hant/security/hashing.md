# 雜湊

## 簡介

`Hash` 門面產生和驗證密碼雜湊，支援 bcrypt、Argon2i 和 Argon2id。密碼輸入為位元組字串。bcrypt 沿用 PHP 的 72 位元組行為。設定 `BCRYPT_LIMIT=72` 可在產生新雜湊時拒絕更長的輸入。

## 設定

使用 `HASH_DRIVER` 選擇 `bcrypt`、`argon` 或 `argon2id`，預設為 bcrypt。在 `config/hashing.rs` 設定 `BCRYPT_ROUNDS` 或 Argon 的 `ARGON_MEMORY`（KiB）、`ARGON_TIME` 和 `ARGON_THREADS`。可呼叫 `Hash::driver(Some("argon2id"))` 選擇具名驅動程式。

### 自訂驅動 {#custom-hash-drivers}

在服務提供者啟動時用 `Hash::extend` 或 `HashManager::extend` 註冊應用自己的驅動。透過 `HASH_DRIVER` 選為預設驅動，也可以把名稱傳給 `driver`。工廠收到管理器並傳回 `Arc<dyn Hasher>`，可透過 `getContainer()` 讀取應用綁定。

```rust
use std::sync::Arc;

use openrust::hashing::HashManager;

pub fn register(manager: &HashManager) {
    manager.extend("project", |manager| {
        Ok(Arc::new(manager.createArgon2idDriver()?))
    });
}
```

管理器在首次使用時建立驅動並依名稱快取。重新註冊後，現有實例會繼續使用，直到呼叫 `forgetDrivers()`。工廠失敗後可在下次查找時重試。遞迴或並行建立正在建構的同名驅動會傳回 `DriverBusy`。透過管理器建立的內建驅動共用應用雜湊限制。

## 基本用法

呼叫 `Hash::make` 產生雜湊，呼叫 `Hash::check` 驗證密碼。`HashOptions::new()` 使用設定中的成本。單次 bcrypt 操作可指定 `rounds`，Argon 可指定 `memory`、`time` 和 `threads`。密碼驗證成功後，用 `needsRehash` 判斷是否儲存替換雜湊。

```rust
use openrust::{
    hashing::{HashOptions, HashResult},
    support::facades::Hash,
};

pub async fn refresh_password(password: &[u8], stored: &str) -> HashResult<Option<String>> {
    let options = HashOptions::new();
    if Hash::check(password, stored, &options).await? && Hash::needsRehash(stored, &options)? {
        return Hash::make(password, &options).await.map(Some);
    }
    Ok(None)
}
```

## 雜湊演算法驗證

應用程式預設啟用 `HASH_VERIFY`。已存雜湊採用其他演算法時傳回 `HashError::AlgorithmMismatch`。移轉期間可用 `HASH_VERIFY=false` 驗證其他受支援演算法的舊雜湊。產生新雜湊時仍使用目前的驅動程式。

OpenRust 在阻塞執行緒上執行密碼操作。每個應用程式預設允許兩個並行操作，容量用盡時傳回 `CapacityExhausted`。已存雜湊或操作參數超出預算時傳回 `ExecutionLimit`。預設上限為 bcrypt 成本 16，Argon 記憶體 256 MiB、迭代成本 10 和 4 個平行通道。可用 `hashing.max_concurrent_operations` 和 `hashing.execution_limits` 設定。
