# Passport

## 安装 {#installation}

已提供 Passport 的配置、密钥生命周期和五张 OAuth 表迁移发布。OAuth 端点和令牌守卫按后续 Passport 单元交付，当前还没有 `install:api --passport`。既有 Sanctum 安装保持独立。

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

## 管理客户端 {#managing-clients}

首批客户端仓储支持查询、创建、更新、秘密轮换和所有权筛选，操作真实 Eloquent 模型。`passport:client` 命令、客户端及令牌撤销、模型关系和完整 scope 配置仍待完成；本批不会启用 OAuth 端点或令牌认证。

在请求或 Artisan 生命周期内解析 `ClientRepository`，容器为每个生命周期提供新仓储。同一参数的重复 `find` 共享实际模型，并缓存未命中；数据库错误传播，允许再次尝试。`findActive` 还检查当前模型的 `revoked` 属性。不要把此作用域服务保存在应用单例中。显式管理的操作可以通过 `Passport::clientRepository()` 创建独立拥有缓存的仓储。

发布并执行客户端迁移后，在上述生命周期内调用以下辅助函数：

```rust
use openrust::foundation::Application;
use openrust_passport::{ClientOwner, ClientRecord, ClientRepository};

pub async fn create_client(
    app: &Application,
    owner: Option<&ClientOwner>,
) -> Result<ClientRecord, Box<dyn std::error::Error + Send + Sync>> {
    let clients = app.make::<ClientRepository>().await?;
    Ok(clients
        .createAuthorizationCodeGrantClient(
            "My application",
            &["https://example.com/callback".to_owned()],
            true,
            owner,
            false,
        )
        .await?)
}
```

参数顺序保留 Laravel 原意。Rust 显式传递默认参数：`true` 创建机密客户端，`owner` 可空，最后的 `false` 不额外启用设备授权。使用 `ClientOwner::forUser(&user)` 从编译注册的 `Model + Authenticatable` 创建所有者，取模型主键和多态类型，不取可能不同的认证标识。公开客户端将机密参数设为 `false`。

`createPersonalAccessGrantClient`、`createPasswordGrantClient`、`createClientCredentialsGrantClient`、`createImplicitGrantClient` 和 `createDeviceAuthorizationGrantClient` 选择对应授权类型。创建记录不等于启用 Password、Implicit 或设备协议端点。`update` 修改名称和重定向地址，`regenerateSecret` 生成新秘密；两者经 Eloquent 保存并返回其布尔结果。`findForUser` 和 `forUser` 排除已撤销客户端，后者按名称排序。`personalAccessClient(provider)` 选择对应用户提供者的最新有效个人客户端，仅当提供者等于 `auth.guards.api.provider` 时也接受空提供者，找不到时返回错误。

机密客户端复用应用已有的 Eloquent 异步哈希服务。创建或轮换的 `Client` 在 `plainSecret` 临时保留 setter 输入，重新读库不会恢复明文。默认 JSON 隐藏 `secret`，不追加明文。只在向所有者交付时复制新生成的秘密，不要记录日志。`record.model::<M>()` 返回的 `Arc<tokio::sync::Mutex<M>>` 指向实际选定模型；`sameInstance` 检查共享对象身份，不代表两行模型的主键相等。

`client.append(&["plain_secret"])` 显式追加该实例的临时明文；重新读库的模型追加后得到 null。追加仍遵守 `makeHidden` 和 `setVisible`。

`ClientModel::hasScope` 和 `ClientRecord::hasScope` 在未设置 `scopes` 或其原始值为 null 时允许所有 scope；已设置时按数组内容检查，成员判断沿用 PHP 非严格比较。默认不开启父 scope 继承。若在当前应用上调用 `passport.withInheritedScopes.store(true, std::sync::atomic::Ordering::Relaxed)`，`orders` 也允许 `orders:read` 等后代。`*` 在这里是普通 scope 名称。开关保存在各应用自己的实例中，调用须位于所属应用的请求或 Artisan 生命周期。

在应用装配时、解析仓储前调用 `passport.useClientModel::<CustomClient>()`。编译期类型实现 `ClientModel` 和 Eloquent `Model`，保留唯一标识校验、全局作用域、事件及转换。格式错误的 UUID／ULID 在解析连接前直接未命中。仓储查询和未绑定实例的 `Client::query()`／`save()` 优先使用模型显式连接，然后是 `passport.connection`，最后是数据库默认连接。创建和读出的模型保留实际连接；随后更改配置也不会把该模型的保存转到其他连接。`Client::queryOn(&connection)` 显式选择连接。事务写入检查模型的有效连接，拒绝不同连接的事务。Rust 的 `resolveConnectionName()` 返回可拥有的名称，以读取应用配置；`getConnection()` 返回实际连接。完整 Client 模型接口、PHP JSON 编码选项及浮点转字符串的精确规则仍待验收。

## 客户端工厂

`Client::factory()?` 创建 `database::factories::ClientFactory<Client>`。可调用 `asPublic`、`asPasswordClient`、`asPersonalAccessTokenClient`、`asImplicitClient`、`asClientCredentials` 或 `asDeviceCodeClient` 应用相应授权状态。后续状态覆盖先前属性。默认生成机密授权码客户端，包含随机 40 字符秘密和一个生成的重定向 URI；所有者为空，revoked 为 false。

`make().await?` 返回未保存模型，`create().await?` 持久化模型。赋值执行模型的异步修改器和转换：新 Client 保留 `plainSecret`，重新读库只包含哈希。`state` 添加应用属性；`intoFactory()` 返回现有 Eloquent 工厂 Builder，以使用数量、关系与回调。

Rust 在编译期选择工厂模型：自定义 `ClientModel` 使用 `ClientFactory::<CustomClient>::new()?`，不动态读取 `useClientModel`。构造时解析并保存模型的有效连接，包括 `passport.connection`，配置错误通过 Result 返回；修改配置后应新建工厂。协议端点与其余 Client 关系仍待完成。
