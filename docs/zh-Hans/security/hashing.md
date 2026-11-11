# 哈希

## 简介

`Hash` 门面生成和校验密码哈希，支持 bcrypt、Argon2i 和 Argon2id。密码输入为字节字符串。bcrypt 沿用 PHP 的 72 字节行为。设置 `BCRYPT_LIMIT=72` 可在生成新哈希时拒绝更长的输入。

## 配置

用 `HASH_DRIVER` 选择 `bcrypt`、`argon` 或 `argon2id`，默认为 bcrypt。在 `config/hashing.rs` 配置 `BCRYPT_ROUNDS` 或 Argon 的 `ARGON_MEMORY`（KiB）、`ARGON_TIME` 和 `ARGON_THREADS`。可调用 `Hash::driver(Some("argon2id"))` 选择具名驱动。

## 基本用法

调用 `Hash::make` 生成哈希，调用 `Hash::check` 校验密码。`HashOptions::new()` 使用配置中的成本。单次 bcrypt 操作可指定 `rounds`，Argon 可指定 `memory`、`time` 和 `threads`。密码校验成功后，用 `needsRehash` 判断是否保存替换哈希。

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

## 哈希算法校验

应用默认启用 `HASH_VERIFY`。已存哈希采用其他算法时返回 `HashError::AlgorithmMismatch`。迁移期间可用 `HASH_VERIFY=false` 校验其他受支持算法的旧哈希。生成新哈希时仍使用当前驱动。

OpenRust 在阻塞线程上运行密码操作。每个应用默认允许两个并发操作，容量耗尽时返回 `CapacityExhausted`。已存哈希或操作参数超出预算时返回 `ExecutionLimit`。默认上限为 bcrypt 成本 16，Argon 内存 256 MiB、迭代成本 10 和 4 个并行通道。可用 `hashing.max_concurrent_operations` 和 `hashing.execution_limits` 配置。
