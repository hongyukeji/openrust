# Passport

## 安装 {#installation}

已提供 Passport 的配置、密钥生命周期和五张 OAuth 表迁移发布。OAuth 端点、客户端和令牌守卫按后续 Passport 单元交付，当前还没有 `install:api --passport`。既有 Sanctum 安装保持独立。

当前应用预留了可选的 `passport` Cargo feature。将它加入 Cargo 默认 features，让 `./artisan` 和普通构建持续启用该包，并保留原有其他默认项：

```toml
[features]
default = ["passport"]
```

包发现会注册 `openrust_passport::PassportServiceProvider`。没有包发现的应用可用 `withProviders` 显式注册。启用 Provider 不会创建表、生成密钥或开放占位 OAuth 路由。

## 部署 Passport {#deploying-passport}

生成默认 4096 位 RSA 密钥对：

```sh
./artisan passport:keys
```

密钥位于应用配置的存储目录，文件名为 `oauth-private.key` 和 `oauth-public.key`。Unix 权限分别为 0600、0660；Windows 使用文件系统访问控制。密钥已存在时退出码为 1。`--force` 替换已有密钥，`--length=2048` 指定长度。替换后重启服务。并发的非强制生成最多产生一对匹配密钥。

```sh
./artisan passport:keys --force --length=2048
./artisan vendor:publish --tag=passport-migrations
cargo build
./artisan migrate
```

五张表的迁移（包括 `oauth_device_codes`）按迁移时间戳发布。Rust 在编译时发现迁移源码，因此发布后先重新构建，再执行迁移。Provider 不自动注册或运行包迁移。重复发布保留已有文件，除非显式传入 `--force`。状态、回滚和重建命令见[数据库迁移](../database/migrations.md)。

## 配置 {#configuration}

包默认提供 `guard = "web"`、空中间件列表，以及可为空的密钥和连接配置；应用配置覆盖包默认值。`PASSPORT_PRIVATE_KEY`、`PASSPORT_PUBLIC_KEY` 支持内联 RSA PEM、字面 `\n` 换行、本地绝对路径或 `file://` 路径（Unix 包括 `file://localhost/`）。相对路径按固定 PHP 本地文件包装器行为拒绝，不会自动绑定到应用根目录。未配置时延迟读取存储目录中的文件。支持 RSA PKCS#1／PKCS#8 私钥及 RSA／SubjectPublicKeyInfo 公钥，不宣称覆盖 League CryptKey 的通用 EC 密钥 API。Debug 和加载错误不包含原始密钥内容。

需要应用编译配置覆盖时先发布：

```sh
./artisan vendor:publish --tag=passport-config
```

在 `config/mod.rs` 中增加 `mod passport;`，并在配置条目中增加 `("passport".to_owned(), passport::load(env)?),`，随后重新构建。这对应 PHP 载入发布配置的机制。发布的加载器与包默认值使用同一份实现。`config:cache` 保存解析后的值；加载新的环境变量前先清理配置缓存。

`PASSPORT_CONNECTION` 为五个迁移定义选择命名连接；null 使用应用默认连接。迁移账本仍放在迁移命令选定的连接，保持 Laravel 职责。迁移显式读取所属应用配置，并发应用不会串用连接名。

在 Provider boot 期间解析 `Passport`，在首次解析密钥服务前设置目录：

```rust
use openrust::foundation::Application;
use openrust_passport::Passport;

pub async fn configure_keys(app: &Application) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let passport = app.make::<Passport>().await?;
    passport.loadKeysFrom(passport.keyPath("keys"));
    Ok(())
}
```

生成前目录须已存在且可写。Rust 使用应用持有的 `Passport` 实例，替代 PHP 的进程全局可变设置，保留 `keyPath`、`loadKeysFrom` 名称。两个密钥独立延迟加载；仅配置成功不表示两者已经配对。Unix 文件权限不属于 0400、0440、0600、0640、0660 时校验失败。应用可显式调用 `passport.validateKeyPermissions.store(false, std::sync::atomic::Ordering::Relaxed)` 关闭该检查，不影响其他应用。内联密钥和 Windows 不检查 Unix 权限位。

默认及标准长度使用现有原生 AWS-LC 生成，其余受支持长度使用独立生成器。RustCrypto 只用于生成与 PEM 解析，不用于在线 RSA 解密或签名；无需安装 PHP 或 OpenSSL 命令行工具。
