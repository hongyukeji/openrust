# Passport-01 配置、密钥与迁移发布

状态：本单元实现及实际验收通过，41 个核定文件本地提交，不推送。只闭合 installation／deploying-passport／configuration 的本批范围，不代表 OAuth 服务端或整个 Passport 完成。

## 固定依据与旧版复用

- [Passport 13.10.0](https://github.com/laravel/passport/tree/f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7)，SHA `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7`：PassportServiceProvider、Passport、Console/KeysCommand、config/passport.php 和五份迁移；保留上游 MIT 许可证。
- [Laravel 13.40.0](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc)，文档固定 SHA `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 passport.md。参考固定 Goravel 1.18 的编译装配，入口／Runner 本批不变。
- 旧版 `../openrust-v0.x` HEAD `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`：检查 `vendor/openrust/passport/src/{provider,migrations,migrations_tests,migrations_mysql_tests,test_keys}.rs` 与 `tests/unit/provider.rs`。提取 load_key／configured_file_path／literal_file_uri_path／read_key_file／key_mode_allowed 的本地路径、换行与权限规则，以及迁移字段和隔离回归；不搬旧 SeaORM／Figment 容器、不保留旧强制公私钥配对要求，不复制旧 Windows 未实现分支。旧版四表扩为固定上游五表，并重新逐字段验证。

## 实现与 Rust 适配

独立可选包 `openrust-passport`，Cargo feature `passport` 默认关闭。包发现注册 Provider；配置与发布使用同一份编译配置。只注册实际实现的 passport:keys，无占位 OAuth 路由、client／install／purge 命令。五份完整 Rust 迁移按上游发布，不自动装载、执行或偷偷建表；应用发布后重建，通过现有编译迁移目录运行。设备表包括在内。

Passport 为应用内实例，保留 loadKeysFrom／keyPath 与 validateKeyPermissions 名称；以 Arc／RwLock／AtomicBool 代替 PHP 的进程静态可变状态，避免并存应用串配置。键服务按应用独立惰性解析，不在 Provider 注册期要求文件存在。公私钥独立加载，允许 inline PEM、字面量\n、绝对路径及本地 file://，拒绝相对文件和其他协议，不暗中拼应用根目录。文件只打开一次，读取内容与权限来自同一文件句柄。CryptKey 的路径返回原生 Option<Path>，inline 为 None；内容使用 Zeroizing，Debug／错误不输出原文。

默认 4096 位及标准尺寸使用既有 AWS-LC 原生生成；其他长度使用 OS 随机源和独立生成器。保留 --length／--force、已存在退出 1、成功 INFO、文件权限 0600／0660。非 force 使用 create_new，竞争测试只产生一对匹配密钥；force 为两文件覆写，不声称事务式原子轮换。拒绝已有非普通文件／符号链接目标。Unix 加载权限白名单 0400／0440／0600／0640／0660，可逐应用显式关闭；Windows 不套 Unix 模式检查。当前支持 RSA 私钥 PKCS#1／PKCS#8、公钥 RSA／SPKI，未交付 League 通用 EC CryptKey 契约。

必要框架配套只有 Output::info／error 组件、ForeignIdColumnDefinition::index，以及迁移连接解析。Migration::getConnection 显式接收所属 Repository，返回 DatabaseResult<Option<String>>；替代旧借用静态字符串，使 passport.connection 能按实际应用返回拥有所有权的名称。Migrator 的 up／down／status 使用相同所属配置；历史账本仍在命令选择连接，表在迁移指定连接。无进程全局配置查找；三语迁移正文同步说明签名变化。

## 实际验收

Rust 1.94.0，macOS ARM64 和 Linux 编译／运行；Linux MySQL **8.4.13**，Composer **2.8.12**／PHP **8.4.14**。日常未运行 SQLite／其他数据库测试；Windows 原生、其他平台／数据库按发布矩阵保留。

```sh
cargo +1.94.0 test --locked -p openrust-passport --test lifecycle
# 下列数据库测试在专属 mysql_acceptance.py 实例中运行，并要求 MYSQL URL：
cargo test --locked -p openrust-passport --test mysql
cargo test --locked -p openrust-framework --test database_migrations mysql_artisan_excludes_named_pending_migrations_per_application -- --exact
cargo test --locked -p openrust-framework --test database_migrations mysql_artisan_status_distinguishes_initial_skips_from_pending -- --exact
# 此事件用例另须 OPENRUST_TEST_MIGRATION_EVENTS=1 和匹配版本原生客户端：
cargo test --locked -p openrust-framework --test database_migrations mysql_artisan_dispatches_migration_and_schema_lifecycle_events -- --exact
cargo test --locked --test console --test feature --test artisan_entry
cargo build --locked --features passport --bin main
python3 -B tests/mysql_acceptance.py -- python3 -B tests/passport_keys_http.py --isolated-source
cargo +1.94.0 clippy --workspace --all-targets --locked --features pail,passport --offline -- -D warnings
cargo +1.94.0 fmt --all --check
npm --prefix docs run build
```

共 **37 个不同测试函数通过**：10 项密钥生命周期（macOS 与最终 Linux 各通过）、1 项真实 MySQL 五表／连接隔离、3 项既有 MySQL 迁移回归、23 项根 Console／Feature／Artisan。10 项覆盖配置保留／force、4096 默认与 2048 覆写、无效长度／写入失败、不泄漏、并发、双应用隔离、保留环境值、加载失败恢复与真实 TCP、Unix 权限和逐应用关闭、file URI／localhost／相对路径，以及三语共用 Rust 示例编译和执行。

参考程序执行固定 Passport 五份 PHP migration 的 MySQL pretend 编译，全部 Illuminate（包括 bus／pipeline）精确 13.40.0；两次输出逐字节一致，结果固化在 schema.json。随后把 PHP 产生的 SQL 与 Rust 迁移均执行于真实 MySQL 8.4.13，逐列类型／默认／空值、规范化索引、无 FK 一致；双应用使用独立前缀，并验证主账本／命名表连接、重复 migrate、rollback 和非法配置。PHP 只执行 DDL 编译，不声称 PHP 服务端／HTTP 完整联调。

实际根二进制在无 .git 的隔离源码中，通过默认 feature 关闭／显式启用、密钥默认／force／已存在／错误／权限、自定义 storage、配置／五表发布及重复保护、应用配置接线后重建、migrate／status／rollback／重建／fresh、config:cache 与 clear。HTTP /、/up 返回 200，/oauth/token 不提前开放，JSON／HTML 404 正确，SIGINT 退出 0 并释放端口。原始 ./artisan inspire、默认无 passport 命令，以及示例 default=["passport"] 后原始 ./artisan passport:keys 入口均实际通过。脚本只允许专属带标签 MySQL，修改 Cargo 默认 feature／config/mod 和生成物均仅限隔离副本。

三语站点构建通过；12 项通用浏览器检查和 6 个语言×根路径章节矩阵（每矩阵访问 Passport／Migrations 两页）通过，覆盖 / 与 /docs/、无页面异常／溢出。35 份源码／测试／文档在最终验收后与隔离副本 SHA 比对一致（提交前脚本仅去除一处行尾空格，token 完全不变）；Cargo.toml 在隔离内有已验收的主动 feature 变化，原始 overlay 保存；33 份入口／bootstrap／应用配置／routes／启动脚本基线不变。未改根 config/mod.rs。

### 排错与未通过范围

最初 PHP reference 的 null PDO 导致 pretend 重连，改为抛错 PDO Closure 后重生成；初次浮动的 bus／pipeline 已固定到 13.40.0。MySQL INFO 文本断言修为匹配实际带句点文本，未改产品输出。专属容器环境值实际为 ID，测试 guard 改为核对容器名称／所有权标签后完整根验收通过。最后一次 Linux 命令误传宿主机 manifest-path，改用容器 /work 后最终 10 项与二进制全过。

事件回归最初没有显式启用开关，0 秒结果属于跳过，不计入通过；打开开关后宿主机 MySQL 8.0.46 CLI 对服务端 8.4.13 的 dump 失败，不能声称该组合通过。改用本批服务器同版本 **8.4.13** 原生 mysql／mysqldump 后完整事件回归实跑通过（1.48 秒）；8.0→8.4 CLI 组合留发布门禁复核，不修改产品掩盖环境失败。

## 依赖与发布边界

Cargo.lock 保留所有既有包版本／校验和，只增加本包和 11 项外部包。rsa **0.9.10** 涉及 [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)（Marvin 私钥运算时序）；本批仅生成／解析／验证，不调用其在线 RSA 签名或解密，标准尺寸生成走 AWS-LC。没有增加 Cargo audit 忽略；不声称完整安全审计通过。后续在线签名须使用经核定实现，发布前 V18 仍须核查受修复版本／可达性和依赖公告。测试专用 schema.composer.json 为执行固定历史原文禁用 block-insecure 阻断，不用于产品运行，不代表依赖安全证明。

OAuth 客户端、授权码＋PKCE、令牌／Guard、设备流程、scopes、install:api --passport 及整合仍在 Passport-02 至 09；V06／07／08／18 未整体关闭。三语 Passport 页面仅陈述当前已过部分，不用占位接口冒充功能。

## 最小提交范围

```text
Cargo.{toml,lock}
vendor/openrust/framework/
├── src/console/mod.rs
├── src/database/database_manager.rs
├── src/database/migrations/{migration,migrator}.rs
├── src/database/schema/foreign_id_column_definition.rs
└── tests/database_migrations.rs
vendor/openrust/passport/
├── Cargo.toml
├── LICENSE
├── config/passport.rs
├── database/migrations/*.rs                    # 固定五份
├── src/{lib,passport,keys,migrations}.rs
├── src/console/mod.rs
├── src/console/commands/{mod,keys_command}.rs
└── tests/
    ├── {lifecycle,mysql}.rs
    ├── fixtures/{schema.json,configure_keys.rs}
    └── reference/{schema.php,schema.composer.json}
tests/passport_keys_http.py
docs/{en,zh_CN,zh_TW}/
├── packages/passport.md
└── database/migrations.md
.agents/project/
├── audits/passport-01/README.md
├── audits/core-closure-ledger.md
├── plan.md
├── current.md
└── progress.md
```

41 文件。原有历史补丁、core/ 和 translation-database/ 未改／未暂存，main／bootstrap／config／routes 未改。计划增量 `ef35334c8` 已提交，当前仅提交本单元。

## 收尾与证据

全部执行结束且确认无 Cargo／Rustc／验收容器占用后，直接删除本批专属本地 target、站点、临时浏览器依赖和上游副本，逻辑字节 **13,259,303,758**；文件系统实际可用增量 **12,147,458,048** 字节，清理后约 **82.53 GB**，两种数字分开记录。本地共享 Cargo registry 和 docs/node_modules 保留。

远程本批目录源码／target／registry／git／Composer／原生客户端等共 **6,335,988 KiB** 已直接删除；镜像清单证明 Composer／MySQL 在批前不存在，核对准确 SHA、无容器引用后删除本批两个镜像，共享 Rust 1.94 镜像保留。数据卷前后一致（空清单），没有 prune／删除卷或共享缓存。远程 Available 105,903,912 KiB。保留日志、两份 Composer／Cargo 版本证据、SHA、原始 overlay 和清理记录在 `/private/tmp/openrust-passport-01-20261119`。

按最新真人授权取消项目人为额度停线；正式 usage 工具当前不可用，不声称实时额度、不启用 credits。原 Goal active，磁盘仍遵守 50 GB 停线，本地范围提交，不推送／改版本／打 tag／发布。下一依赖单元 Passport-02 客户端与仓储。
