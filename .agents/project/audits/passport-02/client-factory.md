# Passport-02 编译绑定客户端工厂

2026-11-20，接续 scope／连接批次。固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 `database/factories/ClientFactory.php`，来源副本及 SHA 保存在本批证据目录。已核对旧版 `v0.x:vendor/openrust-passport/src/client.rs` 的纯模型结构；本批复用当前主线 Eloquent Factory、Faker、Str 随机字符串及异步哈希，不引入旧版模型快照。

## 实现与适配

新增公开 `openrust_passport::database::factories::ClientFactory<M = Client>` 与 `Client::factory()`。默认 owner_id／owner_type 为 null，随机公司名称、40 字符秘密、一个 Faker URL、authorization_code／refresh_token、revoked=false。实现 asPublic、asPasswordClient、asPersonalAccessTokenClient、asImplicitClient、asClientCredentials、asDeviceCodeClient，后状态覆盖前状态；公共与 implicit 状态不生成机密哈希。

工厂委托已有 Eloquent 管线；state 保留包装类型，intoFactory 返回原 Builder 供数量、关系和回调使用。构造返回 DatabaseResult，解析并保存模型有效连接（显式模型连接→passport.connection→默认连接）。自定义类型显式使用 ClientFactory::<CustomClient>::new()，Rust 编译期绑定不动态读取 Passport::useClientModel，也没有实现返回非 Result 的核心 HasFactory trait。这是本批明确的语言适配范围，未宣称动态工厂注册公开面全部闭合；修改配置后重新构造工厂。

发现核心工厂 apply_attributes 直接调用 casts::write_async，跳过自定义模型 fillAsync。最小修复为绑定实际 Connection 后调用既有 forceFillAsync，保留忽略 guarded／异步转换和模型赋值钩子。Client 的临时 plainSecret 因此由真实 setter 维护，不另建明文副本；哈希与临时明文的生命周期仍属于模型。修改前该核心文件无未提交差异。

## 验收

证据目录 `/private/tmp/openrust-passport-factory-20261120/`。从 `02cb18be1` 的 git archive 创建独立源码，加本批 10 份源码／测试／三语正文输入；不叠加共享 ViewData、Session、Encryption、SMTP、Cargo 或 init 在途差异。随后 init 的独立提交不改变本批验收基线。[SHA 清单](factory-source-sha256.json)已逐份与最终隔离输入核对。

- 最低 Rust 1.94：9 个 Client 原生测试、10 个 lifecycle（含真实 Artisan Kernel／HTTP）、12 个 client_lookup、1 个核心 MySQL 工厂回归，共 32 个不同函数通过；单独先跑的工厂状态用例不重复累计。
- MySQL 夹具实际版本 8.4.13，使用项目 tests/mysql_acceptance.py，全部 Passport lookup 函数通过。两个新增测试分别通过真实 TCP HTTP 验证默认属性、六状态、状态叠加、未保存实例、临时明文／哈希、编译模型选择、连接选择和非法配置，以及真实创建→保存→重读→哈希校验；默认客户端表迁移未改变。
- 核心 factories_make_and_persist_related_models_on_mysql 真跑，覆盖现有模型工厂关系、状态、回调及持久化，未执行 SQLite 用例。
- cargo +1.94.0 fmt --all --check 和全 workspace／all-targets／features pail,passport／locked／offline 严格 Clippy 通过。
- 三语文档站构建、根路径与 /docs/ 的三语桌面／移动共 12 项浏览器检查通过，保留既有大 chunk 提示。
- 初次测试夹具直接在 Container::scope 访问 Facade 导致上下文错误，改为真实 HTTP Kernel 请求后复验通过；没有放宽配置或忽略失败。最初编译发现测试访问私有 exists 字段，按公开 exists() 修正。

未重编根 CLI 二进制、未实现 passport:client、未做新的 PHP 运行时工厂 oracle、未验收 Windows 或完整动态工厂选择；现有 lifecycle 实际执行 Kernel 不代替这些项目。全部通过仅指上述范围。

## 后继

Passport-02／V06 继续部分验收，尚需完整 Client 关系、UUID 开关与模型选择机制、Token／RefreshToken 生命周期撤销、完整 passport:client 和其余 PHP oracle。Passport-03 未开始，核心 66/66 口径不变。本批本地提交，不推送；其他协作者 WIP 保留。专属产物清理结果见证据目录 cleanup.json。
