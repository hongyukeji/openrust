# 哈希

启用 `hashing` feature 使用 `openrust::hashing`；`auth` 自动启用同一能力。默认 Provider 登记统一的 `HashManager` 和首次解析的驱动别名。认证、验证码及客户端秘密不各自维护成本配置。

## 配置

编辑 `config/hashing.rs`，部署值放 `.env`。框架与生成应用使用同一组设置：

| 设置 | 默认 | 含义 |
| --- | --- | --- |
| `HASH_DRIVER` | `bcrypt` | `bcrypt`、`argon`（Argon2i）、`argon2id` 或已登记扩展 |
| `BCRYPT_ROUNDS` | `12` | bcrypt 成本 |
| `HASH_VERIFY` | `true` | 拒绝与所选驱动不同的存储算法 |
| `BCRYPT_LIMIT` | `null` | 可选的 make 输入字节上限；null/0 不另加上限 |
| `ARGON_MEMORY` | `65536` | 内存，KiB |
| `ARGON_TIME` | `4` | 迭代次数 |
| `ARGON_THREADS` | `1` | 并行度 |

`hashing.rehash_on_login` 默认 true；它在 Guard 构造时读取，不是每次密码检查时热更新。裸 `BcryptHasher::new` 的 verify 默认 false；裸 Argon 驱动为1024KiB/2次/2并行、verify=false，不能与应用安装默认混淆。类型化配置拒绝错误类型；无效算法参数在对应操作时返回错误，不静默降级。

`hashing.execution` 是通用安全增强，默认内置 bcrypt 成本上限16、Argon内存262144KiB/迭代10/并行4。需要更高预算时在配置中明确调整；元信息、needs_rehash和verify_configuration不因执行预算被改写。内置驱动共享阻塞任务容量，默认2、可由 `HashServiceProvider::with_max_concurrent_operations` 设置1至4；容量满返回503 `password_capacity_exhausted`，取消调用不会提前释放仍在运行的KDF许可。

旧独立 `Passwords`、`PasswordConfig` 与 `auth.password` 已移除。已有Argon2id存储可显式选择 `HASH_DRIVER=argon2id`，并在统一配置中还原成本；默认bcrypt/verify=true不会接受旧Argon记录。verify=false仅用于明确的多算法迁移，须同时处理下文的输入等价关系，不能用它掩盖未知存储格式。

## 生成和检查

```rust,ignore
use openrust::hashing::{HashManager, HashOptions};

let hashing = app.state().resolve::<HashManager>().await?;
let options = HashOptions::new();
let encoded = hashing.make("a secret", &options).await?;
assert!(hashing.check("a secret", Some(&encoded), &options).await?);
assert!(!hashing.needs_rehash(&encoded, &options)?);
```

`HashManager::make`、`check`、`accepts_exact_input` 接受 `AsRef<[u8]>`，可直接传文本、字节切片或字节向量。公开 `Hasher` 的三个明文参数是 `&[u8]`，自定义驱动及 trait-object 调用方应在文本边界显式使用 `as_bytes()`；元信息和已编码哈希继续是文本，不需要转换成 Base64 或有损 UTF-8。

```rust,ignore
let bytes = [0xff, 0x00, b'a'];
// 必须先按应用需要显式选择严格Argon，bcrypt make会拒绝NUL。
let encoded = hashing.make(bytes, &options).await?;
assert!(hashing.check(bytes, Some(&encoded), &options).await?);
let driver = hashing.driver(None)?;
assert!(driver.check(&bytes, Some(&encoded), &options).await?);
```

当业务先检查完整输入能力、再生成哈希时，用一次默认操作选择取得最终具体驱动，后续两步都调用返回的同一个 `Arc<dyn Hasher>`：

```rust,ignore
let driver = hashing.driver_for_operation()?;
if !driver.accepts_exact_input(&bytes)? {
    return Err(openrust::Error::custom(
        "secret_input_ambiguous",
        "所选驱动不能保留完整秘密字节",
    ));
}
let encoded = driver.make(&bytes, &options).await?;
```

`driver_for_operation()` 无名称参数，只选择默认驱动；已识别的嵌套 `HashManager` 也会继续选择至具体叶，每级默认在这次选择中读取一次。能力回调改变 inner 默认时，本次 `make` 仍使用已选叶，下一次操作才选择新的默认。普通 `driver(Some(name))` / `driver(None)` 的具名和缓存行为保持原样；只固定它返回的 Manager 对象，不能替代组合操作对最终叶的选择。唯一示例的 `example secret hash` 使用该入口，拒绝能力不匹配时不再调用make、不输出摘要，也不自动换算法。

普通自定义 `Hasher` 无需实现新 hook，默认视为具体叶；框架通过类型化 `as_hash_manager()` 识别 Manager，不从编码前缀或 Debug 猜驱动。扩展仍须准确声明输入能力并实现一致的秘密处理，隐藏的自变转发不能由框架自动固定。环/创建冲突及解析错误按 `Result` 返回；本入口不负责回收历史非当前默认的缓存所有权图，也不提供通用Manager缓存枚举/清空接口。

通用字节接口不改变登录表单的文本输入协议，也不绕过增强认证策略。bcrypt make 的可选字节长度限制先检查，随后拒绝完整输入中任意位置的 NUL；check 不附加 make 的长度限制。Argon 保留完整字节，包括非法 UTF-8 和 NUL。默认成本、算法校验及计算容量保持不变。

每次 make 使用随机盐。`check` 对 null/空存储值返回 false；启用 verify 时算法不同报错，不伪装成密码错误。显式设置 verify=false 才允许按存储算法验证已有 bcrypt、Argon2i/id，适合受控迁移。历史 `$2x$` 按 PHP crypt 的有符号字节规则验证，不生成该格式的新哈希。

`info` 只返回算法及成本等非敏感元信息，内置驱动对未知格式返回 unknown。`is_hashed` 判断元信息是否识别，不表示当前驱动接受该算法。`verify_configuration` 检查已有哈希能否原样写入：同算法且参数不超过配置上界才接受；它不验证明文，也不等于 needs_rehash。驱动构造或扩展错误通过 Result 传播。

## 认证与输入边界

标准 `DatabaseUserProvider` 使用同一个 manager，按官方 check/make/needs_rehash 处理；密码写回仍有存储字节比较及CAS保护。`HashManager` 对应官方 `hash`，每次操作按当前默认驱动委派；公开 `Hasher` 容器绑定对应 `hash.driver`，首次解析后保持同一驱动实例。更改默认名不会替换已经解析的别名；更改具名配置也不会偷偷重建已缓存的驱动。

bcrypt 的通用语义只使用前72字节，PHP兼容 check 还在首NUL截断。标准 Hash 与标准 UserProvider 保留这个行为；不能把72字节后缀不同误称为两个独立密码。将长Argon密码自动重哈希为bcrypt也会改变输入等价关系，CAS本身无法避免；需要完整字节密码的部署应选择严格Argon，或通过自定义UserProvider/请求校验先实施明确输入策略，不能靠盲目切默认驱动迁移。

显式增强的 `AuthFlows` / `EncodedPasswordHash` 额外保留非空、1024字节上限，并在查账户之前统一调用驱动的 `accepts_exact_input` 能力：bcrypt及允许bcrypt迁移的宽松Argon拒绝超过72字节和NUL，严格Argon保留完整字节。现存与不存在账户采用同一输入检查；严格Argon遇旧bcrypt存储的别名输入或算法不匹配时，执行当前驱动的dummy计算再返回统一认证失败。该策略覆盖这些已识别分支，不保证所有存储成本和外部延迟完全一致。此增强不改变通用 make/check 或标准 UserProvider；自定义驱动若会截断输入，必须在该能力中准确声明。秘密值本身不实现 Debug，也不限制存储必须是Argon格式。

当前增强认证入口尚未接入 `driver_for_operation()`，能力检查与后续哈希操作的驱动选择仍待统一。若自定义驱动或嵌套管理器会动态改变默认配置，不能据上述策略宣称同次完整输入身份已经得到保证；这一已知一致性缺口由认证后继整改，本次秘密文件命令的修复不代表它已关闭。

## 扩展

通过 `HashManager::extend` 登记返回 `Arc<dyn Hasher>` 的工厂；内置名称也可显式覆盖。工厂在管理器锁外执行，普通构造失败可以重试；返回同一管理器的工厂会被拒绝并失效，须显式 extend 更换，释放该工厂已捕获的自引用。捕获管理器时优先使用 Weak；尚未执行的工厂及跨对象循环引用仍由调用方管理。异步依赖可在应用Provider中通过同一容器解析后捕获，不能在同步工厂里阻塞Tokio。扩展须实现公开的HashInfo、HashOptions和BoxFuture契约，不依赖框架私有类型；自定义驱动仍负责自身计算与秘密处理。

本章对应固定 Laravel13.39.0 的 HashManager、HashServiceProvider、三驱动、安装配置及HasherTest。Rust的异步计算、Result及显式执行预算是公开适配/增强，不等于 PHP 运行时或任意第三方算法已实现；当前尚未提供通用Manager的默认名/缓存枚举、清空驱动和Container访问/替换接口；这些不由本章的生成/校验与认证接线代替。

通用Model属性与自定义setter目前仍使用JSON文本值。任意字节的Client秘密赋值、非UTF-8已编码哈希及其元信息/原值保留需要同一属性管线的后继支持；不能先转换为Base64、有损文本或预先哈希来绕过setter，也不能把这部分输入认作已验收。
