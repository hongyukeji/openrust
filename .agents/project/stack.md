# 技术栈

Eloquent 日期属性载体直接声明 `indexmap` 2，当前复用锁文件中的 2.14.2，保持数据库列与属性赋值的插入顺序。原有 JSON 子树继续使用 serde_json 的保形表示。

MB-A01模型Array使用现有同版serde_json的显式raw_value，局部保留JSON数字词法并恢复PHP64类别；preserve_order/float_roundtrip不变，禁止全局arbitrary_precision污染通用Serde与Figment来源。无新增依赖或版本升级；主线、三类生成与适用consumer的限定验收见[模型属性](memory/testing.md#模型属性与认证前置限定验收)。

DB-I01固定 `sea-orm =2.0.4`，仅在仓内源码新增安全 `ExecResult::try_last_insert_id`，SQLite保留有符号rowid，MySQL保留原生u64，不支持的PG结果返回错误；Query层按各固定Processor另作PHP64位数值转换。原因与原包SHA/许可证见 [SeaORM来源](../../vendor/seaql/sea-orm/SOURCES.md)。用户已批准完整必要crate随簇提交，根patch、12个consumer与三类fresh实际metadata均解析同一修复副本；无准备脚本、外部fork或发布替代声明。上游升级时须核修复等价再移除补丁，完整来源/许可证/上游402项SHA保持，当前不为精简改变受验分发。限定验收见[插入标识](memory/testing.md#插入标识与事务执行闭环)。

网络测试库生命周期修复新增可选`shutdown_hooks =0.1.0`（MIT，已取固定crate源码SHA审计，安全接口封装C atexit；无额外依赖）。仅显式网络TestDatabase操作首次调用时登记正常退出回调，等待本进程已派发的有限清理操作；回调只同步计数与条件变量，不执行SQL/访问Tokio/输出日志，不安装信号处理或永久后台进程。原因是async Drop阻塞caller runtime会冻住SQLx已排队的还池任务；不能为此忽略pool.close失败而删除活跃库。SIGKILL/abort等不运行退出钩子，仍需持久归属与下次显式入口补偿；Unix范围已收齐[限定验收](memory/testing.md#网络测试数据库生命周期与持久恢复限定验收)，非Unix私有ACL与副本/无应答服务专项仍开放。

测试数据库持久归属journal复用已有固定rustix1.1.5安全API，Unix的DB feature同时启用`process`以核对真实有效UID及私有文件归属，不信任PID/环境变量。仅用于测试资源归属核验，不给业务数据库增删权限；跨进程补偿按同上Unix限定证据认定。

统一Hashing的PHP `$2x$` 兼容仅按固定PHP crypt生成18个有符号扩展key words（72字节），再交现锁定bcrypt0.19.3验证；不复制轮函数、不写unsafe或调用PHP生产fallback，不新增直接blowfish/base64依赖。24条真实PHP8.5.12正反向oracle保留，成本预算/共享容量仍先于计算；限定向量与集成验证见[Hashing验收](memory/testing.md#统一-hashing-与认证接线限定验收)。

统一 Hashing 前置新增可选 `bcrypt =0.19.3`（Keats/rust-bcrypt，MIT，官方crate API提供随机盐与`$2y$`格式），与既有Argon2 0.6共同由`hashing` feature启用；`auth`依赖同一能力，不为Passport单建密码系统。固定Laravel13.39.0的HashManager/三driver及安装配置为行为依据；PHP长度/NUL/跨算法与配置差分、Provider/生成/唯一示例迁移已收齐[限定验收](memory/testing.md#统一-hashing-与认证接线限定验收)，完整Passport与Model hashed cast仍开放。

`auth-passport` 为显式可选协议基础，复用现有 auth、sha2、base64、subtle、pcre2 与 serde；不新增依赖，不自动注册授权端点。固定参考 Passport v13.10.0/f5f5e9f 与 League oauth2-server9.4.1/9d2f6fc（后者为兼容范围内独立锁定参考，不冒称上游Composer锁定）。完整客户端、签发、存储、Provider与OIDC能力仍按唯一todo实施。

MySQL冷prepare取消使用固定SQLx0.9.0/SeaORM2.0.4及`vendor/sqlx-mysql`同版本临时补丁（MIT OR Apache-2.0），不新增运行期依赖；原始crate文件hash与源码commit见vendor说明。根与生成项目顶层patch必须实际解析到该源，SQLx暂精确固定防fresh生成绕过。Cargo不传播依赖包patch，手写独立消费者须显式同项；正式上游修复/发布取代补丁前保留此构建约束。冷prepare中断废弃单物理连接、回滚结果不伪造；main12、最终43双库/生成/consumer及源图已[限定验收](memory/testing.md#mysql-冷预处理取消与连接淘汰限定验收)。

Fortify限流键使用固定portable-ascii 2.0.2 / 16c17671a804bb92602822113dd91fbc8a35d2af的字符bank与clean数据（MIT许可和逐文件SHA随src/support/strings保存），不是新crate或运行期PHP依赖。Str当前仅提供strict=false转写，Rust有效UTF-8输入边界公开；不以deunicode/ICU近似官方默认，不宣称完整Laravel Str。真实PHP8.5 oracle与完整数据导出保存在本簇证据，Rust差分尚待集中执行。

Fortify生成应用的真实管理HTTP测试同样显式使用既有hmac 0.13/sha1 0.11开发依赖，作为独立RFC客户端输入验证码，不读取框架私有引擎或生成证明。

Fortify/TOTP在途依赖：复用现锁定md-5 0.11（直接别名md5_11，auth-fortify可选）仅实现官方验证码cache键摘要，不用于密码/签名。新增qrcode 0.14.1、关闭默认image依赖并只启用svg，用于本地生成官方双因素二维码，不向远端传secret；像素格式不同于PHP BaconQrCode不改变编码URI，解码回读与192px/SVG颜色另验。共享TOTP密码学primitive与Encryption，auth-fortify为唯一编译开关，旧auth-mfa及其表/流程已删除；应用沿官方Web默认或显式JWT profile装配，当前限定验收与开放边界见testing。

SQLite认证安全事务复用当前SeaORM2.0.4公开TransactionOptions/SqliteTransactionMode，不新增crate或连接池；普通Db事务默认不变。UTC/JSON仍用既有Chrono/serde_json codec，SQLite认证SQL不依赖RETURNING或JSON1。当前依赖身份与来源继续见锁文件及vendor说明，S3b验收状态以唯一todo为准。

Session 的原生数据库载荷复用现有 `base64` 0.22。默认 Console 的 `key:generate` 复用同一 `base64` 和已有 `zeroize` 1.9，两者升为框架常规依赖，供无加密 feature 的应用命令使用。没有新增 crate。SQLx 0.9 物理连接所有权及驻留确认复用 SQLite 迁移已有机制，原生 handler 的驱动错误分类通过 SeaORM 的 SQLx reexport 支持 PG-only 与 session-only 编译边界。

> 重构状态：本文件记录当前依赖资产；[全面重构计划](plans/laravel-rebuild.md) 可按功能簇重新选型，新增/替换后同步这里。计划不等于依赖已安装或新后端已验收。

> 受众：AI Agent
> 更新：2026-10-29
> 摘要：当前依赖与 v1 规划选型分列。已声明版本以各 `Cargo.toml` 为准，实际解析版本以 `Cargo.lock` 为准；下表标为“待实现 / 待引入”的库、feature 和命令均不是当前可用入口。

## 运行时与 HTTP

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| 异步运行时 | `tokio` 1.x | 唯一运行时；启用 `io-util` 支持 HTTP 验收与异步 I/O、`fs` 支持本地缓存异步文件操作 |
| HTTP | `axum` 0.8 + `tower` + `tower-http` | 路由、提取器、中间件；`tower-http` 启用 `fs` 提供静态目录/文件服务，`cors`、`compression-gzip`、`compression-br` 支持跨域与压缩，另有 request-id、trace、timeout、catch-panic；门面直接依赖并导出 `tower::ServiceExt` 供单依赖应用测试路由 |
| TrustedHosts 正则 | `pcre2` 精确 0.2.11（高层 bytes API，MIT OR Unlicense） | 固定 Symfony 模式需要 lookaround、反向引用和 PHP `{...}i` 分隔符语义，现有 Rust `regex` 子集不足；默认不启用 JIT/UTF。`pcre2-sys` 0.2.10 经安全高层调用 PCRE2 C（BSD-3-Clause WITH PCRE2-exception）；本库维持 `forbid(unsafe_code)`，但 native/unsafe 依赖、系统库优先或捆绑 C 编译、`cc`/`pkg-config` 等构建与链接成本需在统一门禁中实际记录。当前仅已声明直接依赖，锁定/实际链接版本、MSRV、跨平台与行为验证待最终集成。 |
| 限流 | `openrust::core::RateLimiter` + `openrust::database::DatabaseRateLimitStore` | 已实现固定窗口服务与双库共享 store，AuthFlows 与通用 HTTP RateLimitLayer/key 策略已接入；gRPC 适配待实现，不额外依赖 governor |
| Cookie | C04-C选用现锁中的 `cookie` 0.18.2作为直接HTTP依赖 | 独立于可选auth/session使用Cookie值与time类型，避免HTTP反向依赖auth；请求队列/wire解析及加密接线已按历史决策 081完成C04-C子范围验收，完整Session/Web栈继续按后继执行。加密复用现Encryptor，不新增密码算法或PHP运行时依赖 |
| 会话 | 唯一 `openrust::session`；复用现 `time`、`getrandom`、SeaORM 与可选 `redis` | 已移除tower-sessions；独立session feature，auth依赖session，session-redis不依赖auth。默认数据库、显式array/custom store，通过SessionServiceProvider装配；40位ID/属性flash与受管理请求生命周期见历史决策 082。D3生成应用仅测试路径显式加入既有`time` 0.3 dev-dependency，用于外部SessionStore实现命名公开签名OffsetDateTime，不新增运行时选型或框架reexport。完整driver/Web生态未全交付 |
| gRPC | 可选 `grpc` feature 使用 `tonic` / `tonic-prost` / `tonic-health` / `tonic-reflection` `~0.14.6`（当前 lock 解析为 0.14.7）、`prost` 0.14、`http-body-util` 0.1；应用代码生成用 `tonic-prost-build` `~0.14.6` | 显式启用 tonic `tls-ring`，并直接声明 `rustls` 0.23 ring 后端以在其他 TLS feature 同时启用时选定进程级 CryptoProvider，避免自动选择歧义；运行时依赖 MSRV 1.88，低于 workspace 1.97。默认构建不链接运行时或要求 `protoc`；生成应用使用 `protoc-bin-vendored` 3.3，不执行全局安装。rcgen 仅供独立 gRPC 示例生成临时测试证书。G2a TLS/mTLS、G2b JWT/PAT/Policy/Status、G2c 健康/reflection/调用预算/资源/有界停机、G3a HTTP/RPC 联合生命周期、G3b proto/trace 兼容及 G3c 双节点 HTTP/2 路由已完成对应验收；真实 L7 验收固定 Envoy 1.40.1。完整集群留在 M6，不自研 RPC 栈，见 历史决策 040 |
| 实时通信 | 可选 `realtime` 启用 Axum WebSocket 与在线 SSE；`realtime-redis` 复用 `redis` 1.8.0 Pub/Sub 跨节点路由 | 本地无 Redis 的 transport 是显式选择；跨节点没有静默本地回退。主包 dev-dependency 与独立验收宿主 `tests/consumers/realtime/` 使用 `tokio-tungstenite` 0.29 做真实 WebSocket 回归；独立宿主另用 `reqwest` 0.13、`futures-util` 与 `bytes` 做双进程 TCP/WebSocket/SSE 及停机验收；Redis 8 服务为验收依赖 |
| OpenAPI | 规划 `utoipa` + `utoipa-axum`，尚未引入 | 类型 schema、统一响应文档与非生产 `/docs` 均待实现 |
| 出站 HTTP | `reqwest` 0.13（Rustls）、`ipnet` 2.11、`httpdate` 1.x | `http-client` 可选 feature；共享连接池、JSON/query/form/multipart/stream、显式超时；默认禁重定向和代理继承；`HttpClient::restricted` 可选执行目标主机/端口、DNS 全结果及公网地址策略，内网例外由主机 + CIDR + 端口显式登记；命名依赖隔离及可选进程内熔断复用此客户端 |
| 外部 OIDC 客户端 | `oauth2` 5.0（禁用 crate 默认 feature）+ 现有 `jsonwebtoken` 11.1（aws-lc-rs） | 可选 `auth-oidc` feature 依赖 `auth`、`http-client`、`encryption`；授权码/PKCE/令牌交换由 oauth2 提供，OIDC Discovery/JWKS 与 ID Token 必需声明校验由框架 glue 完成并通过受限命名 `DependencyClient` 出站；避免引入 `openidconnect` 的受影响 `rsa` 依赖，见 历史决策 053 |
| 模块路径匹配 | `glob` 0.3.3 | 固定laravel-modules对每个显式扫描位置执行POSIX风格glob；保路径顺序、匹配排序及重复来源，不自行实现不完整通配符解析。仅发现module.json，不加载未编译Rust源码。 |
| 序列化 | `serde` + `serde_json`（`preserve_order`、`float_roundtrip`、`raw_value`） | R2 包配置合并需要保留 JSON 对象的插入顺序以对应 PHP 数组键顺序；`indexmap` 已在锁图中。C04-B1 typed JSON 规范化保留原始数值词法以支持Rust u128/i128；不启用全局`arbitrary_precision`；模型Array通过局部`RawValue`保数字词法，保持通用Serde数值与Figment来源表示。修正后仍须覆盖配置、typed请求/scalar/JSON/JWT及无DB feature图。模块JSON发现启用float_roundtrip以对应固定PHP strtod的有限浮点最近舍入，原始整数-0与大整数的PHP类型转换另在发现解析层处理；非有限JSON数值仍明确拒绝并保表示缺口。根集成测试用 `serde_json` 比对包装字段与请求 ID |
| Eloquent 十进制定标 | 复用锁定的 `bigdecimal` 0.4.11 | 固定 Laravel 的 decimal cast 需要任意精度和 HalfUp。复用其大整数实现检查有限十进制分数，保留 MySQL DECIMAL 文本的精度。 |
| 验证 | `validator` | M0 评估 `garde` 后定稿 |
| 国际化 | `openrust::translation`、`serde_json` 与 ICU4X `icu_datetime`/`icu_decimal`/`icu_locale`/`icu_plurals` 2.3.x | 同一 catalog 支持字符串插值、CLDR plural/select；编译数据用于 locale 数字与公历日期格式化。仅启用所需组件；数据随二进制构建，不使用运行时网络。日期时区转换由调用方显式完成；最终二进制尺寸增量纳入 M6 构建基线测量 |
| 配置 | `figment` + 既有 `toml` 1.1（`preserve_order`） | R2-R02启用已有TOML解析器的保序feature，在来源进入Figment前保留表声明序；复用已锁定indexmap，不在框架新增toml_edit依赖或手写解析器。功能接线与验收仍以当前todo为准，不能从feature声明推断有序来源已交付；来源错误继续按可信schema路径脱敏 |
| 日志 / 追踪 / metrics | `tracing` + `tracing-subscriber`；可选 `metrics` feature 使用 `metrics` 0.24.6 + `metrics-exporter-prometheus` 0.18.3；可选 `otel` feature 使用 OpenTelemetry SDK 0.33.1 与 OTLP HTTP exporter | `/metrics` 仅在配置开启后注册；HTTP 指标标签为固定方法与状态类别。HTTP、gRPC 和 worker 复用已校验的 `traceparent`；配置 OTLP endpoint 后由 SDK 后台线程导出 spans，使用同步 HTTP client 适配其执行模型。真实 Collector 已验证 HTTP 与 HTTP→gRPC trace 导出；性能和完整 workspace 最终门禁仍在 M5 验收中 |
| 时间 | `chrono` + `chrono-tz` 0.10.4（编译数据标识 IANA 2025b），日期输入与队列 cron 使用 | C04-B2将既有chrono-tz改为普通依赖，不新增crate，使请求日期不依赖queue feature；构建/产物包含静态时区数据，集群节点须运行相同构建产物。chrono-tz生成表在2099年后采用固定尾偏移，不能单独证明PHP未来时区等价；C04-B2的POSIX尾规则已实现，原始与未来时区参考、598 footer及适用真实入口验收通过；不据此声称全体日期字符串已穷举。PHP参考运行器tzdb标识2025.2与Rust数据标识分列记录，行为仍须实际差分 |
| 日期解析来源 | 固定 PHP 8.5.0 timelib、Carbon 3.8.4 与 PHP `math.c` 的 Rust 实现 | 不增加 PHP 运行时或 Cargo 依赖；timelib/Carbon 为 MIT，移植的 `number_format` HALF_UP 分支保留 PHP License 3.01 及版权。固定源码 SHA、再生成和边界见 [SOURCES](../../vendor/openrust/framework/src/support/calendar/date/SOURCES.md)，完整授权原文随日期源码保留；Cargo deny 不代替这些非 crate 来源的许可记录。行为验收仍按 C04-B2，不以来源记录声明完整日期等价 |
| cron 解析 | `cron-parser` 0.12（`queue` feature） | 只接受五字段表达式；可复用解析结果计算日历触发，DST gap 跳过且 fold 的两个绝对时刻有序返回，框架以本地 wall-slot 唯一键折叠重复分钟 |
| ID | 已提供可选 UUIDv7，详见下表 `db-uuid` | 默认整数 ID；生成选项为 `--uuid-id`，不使用不存在的 `--uuid` |

Session默认Cookie名的内部转写数据固定参考 `voku/portable-ascii` 2.0.2（commit `16c17671a804bb92602822113dd91fbc8a35d2af`），仅纳入默认英文路径可达的1468项映射，不新增Rust或PHP运行时依赖。Laravel仅声明兼容版本范围，不能把此参考固定值称为其已锁定解析版。映射、再生来源与完整许可位于 `src/session/name/` 及 `name.rs`；23项PHP名称参考结果与Rust名称测试按各自范围核验，C04-D1适用集成门禁已通过；这不等于完整PHP Session oracle。

## 数据

| 能力 | 选型 | 说明 |
| --- | --- | --- |
| ORM / 迁移 | `sea-orm` 2.0.4 + `sea-orm-migration` 2.0.4 | 采用 M0 双库迁移 spike 验证的版本；tokio Rustls 默认运行时，OpenRust 官方连接驱动按 `db-postgres` / `db-mysql` / `db-sqlite` feature 开启，SQLite当前仅连接基础限定范围。外部自定义插件可通过同版本 SeaORM 依赖的 feature 统一选择其他 backend；SQLite 通用 `Db` 与应用自有迁移示例见 `tests/consumers/db-custom-sqlite/`，不代表 OpenRust 框架迁移、认证/队列或方言的完整支持 |
| SQLite 最低版本依赖修正（限定验收） | 固定官方 `sea-orm-migration` 2.0.4 的仓库内副本 `vendor/sea-orm-migration` | 上游内部 SeaORM 依赖隐式 defaults 启用 SQLite RETURNING，3.26 实测连接被拒绝。副本仅在该依赖显式 `default-features=false`，源码/测试原样；来源、补丁和许可见副本 SOURCES。workspace 直接 path 传递，生成应用继续唯一 OpenRust re-export，不使用不可传递的根 patch。SeaORM 其他此前有效 defaults 在 workspace 显式保留，避免丢 codec/stream 行为。不得额外直接引官方迁移包重新统一 RETURNING；12个独立consumer与生成依赖图均为单一迁移来源，真实3.26连接/迁移子范围已验；未来 crates.io 分发另验，不等于已发布或已支持全 SQLite。 |
| 数据库 URL | `url` 2.5 | `testing` 替换隔离数据库名时保留连接凭据和查询参数 |
| SQLx 支持 | `sqlx` 0.9 | MySQL driver 强制 TLS（Rustls）；不启用存在安全公告的 RSA 非 TLS 认证路径；`db.sqlx()` 尚未实现，若提供需先定义公开 API、feature 与连接生命周期 |
| 数据库 | Laravel兼容目标：MariaDB 10.3+、MySQL 5.7+、PostgreSQL 10.0+、SQLite 3.26.0+、SQL Server 2017+；日常实际数据库验收仅MySQL8.4 LTS | 目标不等于当前支持。现版本门禁仍PG14/MySQL8/SQLite3.26，须连同驱动/SQL差异整改，不能只降低阈值。其他类型与最低/当前版本运行统一发布前完整适用验收；未实现或未验为发布缺口。详见[兼容基线](plans/database-compatibility-baseline.md)和[运行政策](memory/testing.md#当前数据库运行政策)。 |
| UUIDv7 主键 | `uuid` 1.x（`v7`, `serde`），通过 `db-uuid` feature 同步启用 SeaORM/SeaQuery UUID codec | 仅在模型生成显式使用 `--uuid-id` 时启用；默认仍为自增整数。UUIDv7 由生成实体的 SeaORM `before_save` 在应用进程生成 |
| Redis | 暂定最低支持：Redis 6.2+；日常开发/Compose：Redis 8；当前真实服务验收使用 Redis 8.10.2 | 缓存与可选 Session adapter 已通过 Redis 8.10.2；Redis 6.2 最低版本及最终集群兼容仍需发布前专项验收 |
| 密码哈希 | `argon2` 0.6（Argon2id / PHC） | RustCrypto 实现；随机 salt；参数有界可配置；哈希与验证移入 Tokio blocking pool |
| 认证与限流 key 摘要 | `sha2` 0.10 + `subtle` 2.6 | Session、PAT、CSRF 与数据库限流 key 存 SHA-256 摘要；CSRF 摘要比较使用常量时间比较。限流摘要仅避免原文持久化，不抵抗低熵 key 猜测 |
| JWT | `jsonwebtoken` 11.1（`aws_lc_rs`） | 复用成熟 JWT 编解码与验证；避免 `rust_crypto` 带入被安全公告覆盖的 `rsa 0.9`；OpenRust 固定 HS256 并自行验证配置、kid、claims、密钥轮换与刷新凭据语义；MSRV 声明为 1.88，须由 workspace 1.97 验证 |
| TOTP | `hmac` 0.13 + `sha1` 0.11 + `subtle` 2.6（仅 sha1 由 `auth-fortify` 启用，hmac/subtle 同时服务基础 URL 签名） | 按 RFC 6238 实现 HMAC-SHA1 动态截断与常量时间码比较；锁文件中已有对应版本，新增直接依赖用于显式协议实现与 RFC 已知向量测试；seed 继续由现有 `Encrypter` 加密 |
| 应用数据加密 | `aes-gcm-siv` 0.12.1（AES-256-GCM-SIV / RFC 8452）、`getrandom` 0.3、`base64` 0.22、`zeroize` 1.9.1 | `encryption` feature 固定 32-byte key、随机 nonce、认证 envelope 与 key ID 轮换；显式启用 AEAD crate 的 `zeroize` feature；不复用 JWT 签名密钥 |
| Laravel URL 签名 | `hmac` 0.13 + `sha2_11`（`sha2` 0.11 别名）+ `subtle` 2.6 | 基础路由提供 HMAC-SHA256 URL 签名及常量时间校验，不依赖可选 JWT signing feature。hmac 0.13 与 sha2 0.11 同用 digest 0.11，现 sha2 0.10 调用方保持原版；MFA 仍为 HMAC-SHA1。默认及 no-default 的实际编译与依赖成本待本簇验收，不因锁内已有版本冒称零成本 |
| Laravel 字符串载荷 | 复用锁定的 `aws-lc-rs` 1.18.2、`base64`、`getrandom` 与 `zeroize` | 框架仅在 `encryption` feature 引用 AWS-LC 非 FIPS CBC PKCS#7/GCM/HMAC 原语；随机 IV/nonce 沿现有可失败系统随机入口。 |
| 签名链接 | 既有 `jsonwebtoken` 11.1、`base64` 0.22、`getrandom` 0.3、`sha2` 0.10 | 可选 `signing` feature 固定 HS256 和用途专属 key ring；逐条撤销用 PostgreSQL/MySQL 共享存储；未新增依赖 |
| JWT、锁与幂等 owner randomness | `base64` 0.22 + 必需依赖 `getrandom` 0.3 | 操作系统 CSPRNG 生成 JWT jti、refresh credential、共享锁、请求幂等 owner token及Session ID；共享锁令该依赖不再按认证/队列/加密 feature 可选。启用std错误集成；Session随机错误仍转安全内部I/O错误，独立consumer不依赖根dev feature统一 |

标准宿主构建预整合既有 `db-mysql`、`cache-redis`、`queue-redis` feature，不引入新依赖或版本。生成项目显式登记所选驱动和队列 feature，库的 `default-features=false` 精简路径保留；编译能力、Provider 登记与实际连接分别处理。本簇适用门禁、真实服务与fresh已通过限定验收，完整后端/生产范围仍见唯一 todo。

## 异步能力与 driver

| 能力 | 当前实现 | 待实现边界 |
| --- | --- | --- |
| 队列 | `queue` feature 下默认使用数据库 driver（历史决策 003，不依赖 `apalis`）；可选 `queue-redis` 复用已有 `redis` 1.8.0 | Redis Queue 已通过 Redis 8.10.2 多连接、唯一派发、失败重试、租约恢复与 Redis-only server 集成验收；至少一次及事务/任务组限制见 历史决策 046，Redis 持久性/复制与最低版本仍由部署和 v1 发布门禁负责 |
| 调度 | `queue` feature；`Scheduler` + interval/UTC daily/五字段 cron，`cron-parser` 0.12 与 `chrono-tz` 0.10.4 | F1 已实现；cron 使用 IANA zoneinfo 表，DST gap 跳过、重复 wall minute 去重；有界补跑及双库验收见 历史决策 037 |
| 缓存 | 根 `openrust::cache` 提供 Cache、命名 stores、memory/file 与公开 custom driver contract；Redis 由 `redis` 1.8.0、`connection-manager` + `tokio-rustls-comp` feature 提供 | 根 package 默认启用 `cache-redis`，与标准应用 Redis store 配置一致；`--no-default-features` 仅保留未编译 Redis driver 的库构建，显式 Redis 配置启动时拒绝，不回退 memory；Redis 6.2 最低版兼容仍待专项验收 |
| 请求文件解析（C04-F分支限定交付） | Axum 0.8.9启用官方`multipart`，仅新增锁定`multer` 3.1.0（MIT、未声明MSRV，本簇Rust1.97适用检查已通过）；既有`tempfile` 3.27作为Request基础依赖，取消仅data-exchange-tasks启用的门槛 | 不手写multipart协议或安全临时文件；沿唯一Request提取与所有权/取消机制。既有`http-body-util`从grpc专属提升为基础直接依赖，用`LengthLimitError`区分413与底层I/O错误，未引入新版本。multipart上限与公开行为已完成C04-F任务分支限定门禁、生成应用、真实MySQL/TCP验收，已进入未提交main组合候选、尚未完成组合门禁；Rust1.97适用检查与deny已通过，不把它等同全部平台或完整MIME/Storage验收。当前锁仅新增multer，无既有第三方版本升级。 |
| 存储 | OpenDAL 0.59.x（当前锁定 0.59.5）提供 filesystem/S3-compatible I/O；`bytes` 1.12 与 `futures-util` 0.3 承载有界流 | 根模块 `openrust::filesystem` 提供配置、流检查和 driver contract；默认不启用，`storage-local` 启用文件系统，`storage-s3` 增加 S3 服务和 Rustls HTTP transport；OpenDAL facade 关闭默认 feature，仅开启相应服务。S3 签名依赖链使用 `tiny-keccak`（CC0-1.0），许可在 `deny.toml` 中显式允许 |
| S3 ACL（ST-V main限定集成） | 可选精确 `aws-sdk-s3` 1.152.0；关闭默认feature，仅 `rt-tokio/default-https-client/http-1x`，随 `storage-s3` 启用 | OpenDAL 0.59.5未公开对象ACL读取/修改；SDK用于GetObjectAcl/PutObjectAcl及最终CopyObject显式ACL，仍复用唯一Storage/现暂存流。官方声明MSRV1.94.1与Apache-2.0；当前锁图、Rust 1.97双feature检查与deny已在main验收；真实ACL启用/禁用bucket和迟到CopyObject仍待服务证据。不得读取系统凭据链、吞ACL错误或将可见性等同公网可达；真实范围见本簇Spec，未宣称真实ACL服务已验收。 |
| Unix受限文件操作（ST-V main限定集成） | 可选精确 `rustix` 1.1.5，`default-features=false`、`fs/std`，仅Unix依赖且随storage-local启用 | 复用现锁定版本的安全dirfd/openat/fstat/fchmod/renameat接口，修复可见性修改硬链接与目录替换竞态；不写unsafe/手造syscall。std用于安全OwnedFd与标准文件互转；本地/公开driver/真实TCP限定行为已验收，其他平台与完整Filesystem仍开放。 |
| 邮件 / 短信 | `lettre` 0.11 + log/fake；`mail-smtp` 启用 Tokio/Rustls STARTTLS，仅本机 loopback 可显式关闭；`sms-twilio` 复用 `http-client` 与 `base64` | 本机 Mailpit SMTP 实收已有验收记录；真实 Twilio 投递待授权账号和目标，log/fake 不能代替送达证据 |
| 模板 | MiniJinja `~2.24.0` 关闭默认 feature；邮件使用 `mail-templates` 编译期模板，服务端页面使用可选 `view` feature 及其 `loader` 能力 | 邮件继续使用受信任编译期资源；视图使用应用根目录 loader、严格变量、HTML 自动转义、燃料上限和可选开发重载；不读取网络或用户上传模板 |
| 通用数据交换 | 可选 `data-exchange` 使用 `csv` 1.4、`calamine` 0.38、`rust_xlsxwriter` 0.101、`zip` 9.x 与 `quick-xml` 0.42 | CSV 逐记录处理；XLSX 读取使用 cell streaming，写入启用 constant-memory；限制 ZIP 展开、shared strings 数量和表格行列/单元格。CSV 为 MIT OR Unlicense；其余依赖 MIT 或 MIT OR Apache-2.0；组件 MSRV 1.73/1.88，均低于 workspace 1.97；压缩、加密等非必要 zip feature 关闭；calamine 0.38修复[RUSTSEC-2026-0436的CFB解析拒绝服务漏洞](https://github.com/tafia/calamine/pull/762)，其MSRV仍为1.88 |
| 数据交换任务临时文件与导入标识 | 可选 `data-exchange-tasks` 使用 `tempfile` 3.27 和现有 `uuid` 1.x，并复用 Queue、Storage、Db 事务与 RequestIdempotency | 输入/输出暂存使用操作系统临时目录的自动回收文件；UUIDv7 用于服务端生成导入标识，仅显式启用该任务能力 |

## 工具链

| 用途 | 当前状态 |
| --- | --- |
| CLI 解析 | 已用 `clap`（derive） |
| 模板渲染（生成器） | CLI 继续使用内置模板文件与 Rust 字符串生成；应用页面和邮件 renderer 分别由 `view` 与 `mail-templates` feature 提供 |
| 生成应用验收专用依赖 | gRPC 生成测试使用现有 `rcgen` 0.14.10、`tempfile` 3.27 和 Tokio process/net/io/time；Fortify 生成测试使用与框架同主版本的 `hmac` 0.13、`sha1` 0.11、`url` 2.5。分别按 `--grpc`、`--auth` / `--fortify` 或 `--profile jwt-api` 选择写入生成项目 dev-dependencies；测试用临时 CA、真实二进制联合监听和测试端 RFC 6238 验证器，不添加生产认证算法或全局证书配置。 |
| 文件监听（`openrust dev --watch`） | macOS 和 Linux 开发入口使用 Watchexec 2.8 的 JSON 事件与 Cargo 构建消息。CLI 复用锁定的 `rustix` 1.1.5 安全进程接口发送信号并回收进程组，使用现有 `tempfile` 3.27 分离并回收运行产物 |
| 快照测试 | 已用 `insta` |
| TOML 配置编辑 | CLI 使用 `toml_edit` 在整套源码 workspace 中合并应用成员，并保留根 `Cargo.toml` 的其余内容与注释；不引入额外 manifest 重写器 |
| 流式请求测试 | 已用 `tokio-stream` 0.1（dev dependency）构造出站 HTTP 一次性 stream；不进入默认运行时依赖 |
| 数据库隔离测试 | 已用 `testing::TestDatabase`，提供 PostgreSQL 模板克隆与 MySQL 独立库迁移 |
| 过程宏 | 已用 `syn` + `quote` 解析控制器 impl 项并生成 `Resource` 实现 |
| 假数据 | `fake` 尚未引入；已有 Mail/SMS/Audit fake 是各能力自身测试实现，不依赖该 crate |
| 依赖审计 | 当前验证使用 `cargo-deny`；`cargo-audit` 为规划工具，不等于已执行 |
| 语义化版本检查 | M6 对已有源码版本做 API 兼容性检查；使用 `cargo-semver-checks` 时明确源码 baseline，不以 crates.io 已发布包为前提 |
| 变更日志 | `git-cliff` 为规划工具 |
| 文档站 | mdBook 0.4.52，首批指南位于 `docs/`；固定 Laravel 主导航、Modules 独立扩展组。验收使用项目内官方预编译工具，校验发布资产 SHA256，不全局安装；后续章节随功能补齐 |
| CI | GitHub Actions；常规MySQL8.4 service集成，手动release-backends包含PG18子矩阵；后端专属风险另按影响定向，CI子矩阵不等于完整发布验收 |

## 版本政策

可选快速开发模块 端侧规划：Admin Web 使用 pure-admin 精简国际化版 + Element Plus（Vue 3 / TypeScript / Vite 路线），App 使用 uni-app；PC 端仅设计，暂不引入运行时。B1 锁定具体兼容版本、App 实现代际/渲染模式与目标平台，记录实际依赖和锁文件；目前这些前端依赖尚未安装，不属于 v1 Rust 核心依赖。范围、选型依据及验收见 [可选快速开发模块 计划](plans/optional-rapid-development-modules.md)。

Edition 2024；MSRV 为 stable 减两个小版本；0.x 期间破坏性变更只在次版本号；依赖升级随 Dependabot，升级主版本写 ADR。

## 已确认但尚未引入的底层补强

底层补齐计划 F1–F10 是目标范围；具体版本、MSRV、许可证、安全维护和构建成本在实施 Spec 核实后登记。F1 使用 `cron-parser` 0.12 与 `chrono-tz` 0.10.4；F2 使用 ICU4X 2.3.x 编译数据组件，cargo feature 只启用日期、数字、locale 与复数能力，不引入运行时数据服务。

- cron/时区、i18n 复数/地区格式化、邮件与生成模板优先成熟组件；候选名称仅供评估，不能当作已锁定选择。
- OAuth2/OIDC、TOTP、CSV/XLSX 使用成熟协议/格式组件；复用本地认证/密钥、Queue/Storage，不自研协议和文件格式。
- cache tag/SWR/加载合并、查询规则、租户作用域与 fixtures 优先在既有能力上整合；只在有明确附加价值时引入依赖，不建立新 ORM/容器。

## R1 Cargo 布局

根 openrust package 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs，modules/example 业务模块通过公开 API 使用框架；工具保留独立 package。没有新增依赖或依赖升级；默认 feature 改为 `cache-redis` 以满足标准配置，显式 `default-features=false` 的消费者仍可关闭它；MSRV 保持 1.97。

根框架库、server 与工具共用根 Cargo.lock，独立 consumer/生成应用拥有自己的锁图，本轮只调整本地 package 身份/路径解析，不升级注册表依赖。框架根无需额外包别名或包装 crate；公共导入、宏与 Rustdoc 都保持 openrust。

SQLite连接基础：db-sqlite复用既有SeaORM2.0.4/SQLx0.9的SQLite驱动，libsqlite3-sys0.37的bundled编译仅新增既有cc依赖边，未升级版本或启用sqlite-use-returning-for-3_35；cargo-deny全特性四项通过。实际引擎3.51.3及compile_options已保全。固定官方最低3.26.0保留且完整运行矩阵未验；PDO默认busy_timeout60000ms与逐物理PRAGMA已验，默认不强制WAL/NORMAL。完整迁移/业务SQL与配置边界仍待后继，见唯一todo及testing。

SQLite S2在途依赖：optional rusqlite精确0.39.0、default-features=false，仅db-sqlite编入。其libsqlite3-sys ^0.37与SQLx现0.37共享唯一C引擎；用于安全fresh内存连接探测真实原生外键默认，SQLx0.9公开API不能移除初始外键PRAGMA，compileoption宏存在也不能区分宏值0。不是第二业务Provider/池或新engine；不启用rusqlite bundled/modern/full/cache或提高SQLite最低版本。源码包SHA与官方元数据在tmp/sqlite-builtin-20261102/s2/references，尚未运行验收；全局auto-extension动态重配/按VFS或filename定制初始化仍须明确边界。

SQLite迁移锁在途：db-sqlite同时启用既有rustix1.1.5（Unix NOFOLLOW安全sidecar打开），Windows仅启用winapi-util精确0.1.11安全读取打开文件身份/链接计数；复用已有锁图版本，不在库写unsafe。普通文件sidecar锁与原生DDL池分离，防独立SQLite写锁自锁；硬链接共同数据库路径仍是未闭合项，不能凭identity hash或canonicalize视为已解决。

S3b测试工具接线：tools/testing 的 db-sqlite 显式转发既有根 openrust/db-sqlite，并重导出同一TestDatabase；无新依赖或锁文件变化。生成项目实际依赖该feature，不能只以根框架SQLite测试代替调用方验收。

SQLite新应用自动迁移前的部署密钥复用CLI既有getrandom生成32字节Base64，各用途独立；不新增密码学依赖或改变缺密钥启动拒绝规则。

AR-P01测试资源scope：框架测试复用同一SQLx0.9.0的sqlite类型feature，仅供不连接数据库的options断言；OpenRust的db-sqlite运行feature仍只在发布多库scope开启。纯FS测试在Unix复用rustix=1.1.5（fs/std/process）、Windows复用winapi-util=0.1.11测试依赖；不新增版本或生产默认后端。原PG NUMERIC wire与Session期限算法拆为生产/纯测试共享叶，不复制算法。

AR-M01新增本地workspace包`openrust-modules`，从框架迁移模块/config/activator/命令实现，复用既有serde、serde_json、glob、tokio、toml及开发figment/tempfile依赖，无新增registry依赖版本。框架不依赖该扩展，根应用默认集成；AR-M01已由6c21bab0限定验收提交，完整模块契约仍沿唯一todo推进。

I18N-S01在途依赖：新增本地workspace扩展openrust-translation-database，应用仅显式translation-database feature启用，三个既有DB feature只弱转发后端；扩展default=[]，依赖既有framework、serde/serde_json、sha2、tokio，开发测试复用figment，无新registry版本。无feature限定的安全FileLoader在Unix需既有rustix1.1.5的安全open API，因此framework将该target依赖由optional改为常规，移除旧dep:rustix feature边，既有fs/std/process集合不变；不新增业务池、驱动默认或第二Translator。官方锁解析仅新增本地扩展包与根边、registry记录未变，Root已核准；运行输入已冻结，编译、生成和实库待集中验收。

Source的Unix特殊文件测试以系统POSIX `mkfifo -m 600`构造临时FIFO；工具启动或退出失败明确报错，不跳过安全断言。仅测试夹具调用该命令，生产FileLoader仍使用已固定rustix安全文件接口，无新Rust依赖、unsafe或运行期命令。
