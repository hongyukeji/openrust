# Passport-02 客户端查询与写入首批

状态：客户端查询与写入首批实现、实际验收通过。本批只核定客户端查询、创建、更新、秘密轮换和所有权筛选；Passport-02 整单元仍未闭合，不能据此推进依赖完整客户端管理的 OAuth 协议交付。

## 固定原文与旧版复用

- [Passport 13.10.0](https://github.com/laravel/passport/tree/f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7)：Client、ClientRepository、Scope、ResolvesInheritedScopes 及 ClientRepositoryTest。方法顺序、grant 种类、默认值、秘密 setter、UUID 查询门禁、once、所有权和 personalAccessClient 筛选逐项核对。
- [Laravel 13.40.0](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc)：Eloquent Model／HasAttributes，文档固定 SHA `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 passport.md `managing-clients`。Goravel 1.18 沿用既定编译装配参考，本批不改应用入口或 Runner。
- 旧版工作树 HEAD `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`，查阅 `vendor/openrust-passport/src/{client,client_tests,php_array,repository,repository_cache,scopes}.rs` 和 framework support/number.rs。提取数组形状、标量比较、实际模型共享、错误不缓存和请求作用域缓存规则。旧仓储只有读取子范围且未接完整 Eloquent 保存，未照搬成完整仓储。

## 本批实现与语言适配

- Client 的私有赋值代理和外层模型共用唯一 ModelState；代理只复用属性转换，不作为第二种模型保存、发事件或查询。秘密通过现有 Eloquent 异步 hashed cast／应用哈希线程池处理，plainSecret 用 Zeroizing 保留 setter 输入。默认 JSON 隐藏哈希，不追加明文；读库模型没有临时明文。
- ClientRepository 使用当前容器的 scoped 工厂，根容器预先解析的仓储不会进入后续请求缓存。相同参数的并发查询由 OnceCell 合并，保留同一实际模型和未命中；查询错误可重试，findActive 每次检查实际实例的 revoked。没有进程级结果缓存。
- ClientRecord 在 Arc／异步 Mutex 中持有真正的编译注册 Model，可取得同一类型的模型引用。不会把模型替换为 JSON 快照；sameInstance 只表示对象身份，不冒充 Eloquent 行身份比较。异步 Mutex 防止 Rust 并发数据竞争，保持 PHP 同请求实例可变语义。
- Passport 应用实例保存选定模型的编译工厂，useClientModel 在解析仓储前配置。自定义模型的唯一标识、全局作用域、Retrieved／Creating 事件和转换保留。仓储操作优先显式模型连接→passport.connection→数据库默认；记录后续保存使用原连接。错误标识在解析配置／连接前直接未命中。
- 创建六种 grant 客户端；AuthorizationCode 可附加设备 flow。Rust 将 PHP 可选参数显式传入，参数顺序不变。创建遵守 Eloquent forceCreate 的事件／布尔结果语义，update 和 regenerateSecret 返回实际 save 结果。创建记录不代表对应 OAuth 协议已启用。
- ClientOwner 从实际用户 Model 的主键和 morph class 取身份，不取可能不同的 auth identifier。forUser／findForUser 保留未撤销筛选和 legacy user_id／现代 owner_id＋owner_type；forUser 按 name 排序。返回支持库 Collection<ClientRecord>，以容纳运行时选定的编译模型，元素仍为实际模型。
- personalAccessClient 保留默认 provider 的 NULL 回退、最新创建时间和逐模型 hasGrantType 判断；无匹配时返回原错误文本。必要框架接线只有 getCreatedAtColumn（验证自定义 born_at）和 doc(hidden) PHP 标量比较复用出口。Cargo.lock 只给既有 Passport 包增加 serde，未升级旧依赖。

## 验收

隔离源码：`git archive d5d078877` 加本批 18 个源码／测试／正文文件，不包含共享工作区 init、Session、Encryption、SMTP 或其他在途改动。最低 Rust 1.94.0、macOS ARM64，复用本单元独占 target，禁用 incremental 与 debug symbols。[源码清单](source-sha256.json)记录 18 文件一致性和 34 个未变入口／bootstrap／config／routes／Cargo 基线。

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 clippy --workspace --all-targets --locked --features pail,passport --offline -- -D warnings
cargo +1.94.0 test --locked -p openrust-passport --test client --test lifecycle
python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --locked -p openrust-passport --test client_lookup -- --test-threads=1
cargo +1.94.0 build --locked --features passport --bin main
./artisan inspire
npm --prefix docs run build
```

实际 MySQL 使用远程专属标准 mysql_acceptance.py 包装器、MySQL 8.4.13 和 SSH 回环隧道，随机凭据只在环境中传递。每次建独立标签的 tmpfs 实例，退出后回收容器，不建／删数据卷。最终日志和原始脚本保留于 `/private/tmp/openrust-passport-02-client-20261119/`。

验收矩阵：

| 范围 | 结果 |
| --- | --- |
| 原生 Client／数组／Scope 6 项 | 通过；真实应用哈希服务，未执行 SQLite SQL |
| 既有 Passport 生命周期 10 项 | 通过；密钥、发布和双应用 HTTP 等受影响回归 |
| MySQL client_lookup 8 项 | 全部通过（96.20 秒）；含真实 MySQL 新式／legacy 表、模型事件、创建／更新／轮换、全局作用域、错误重试、并发查询、缓存、所有权、最新 provider 选择、自定义时间列、双应用真实 HTTP 与文档示例 |
| 当前实际二进制 | 启用 Passport 的 inspire、passport:keys 帮助；原始 ./artisan inspire 退出 0 |
| 根 HTTP | `/`、`/up` 200；API／Accept JSON 404 为 JSON，普通 Web 404 为 HTML；SIGINT 退出 0、端口释放 |
| Rust 检查 | 整仓 fmt 和 features pail,passport 全工作区全目标严格 Clippy 通过 |
| 三语正文 | 同一 Rust 示例在真实 MySQL 中执行；隔离三语站构建通过，Vite 大 chunk 提示保留 |
| 浏览器 | 12 项通用及 6 项 managing-clients 矩阵通过，覆盖 `/` 与 `/docs/`、三语、移动／桌面、无页面错误／横向溢出 |

首次增强测试使用 2040／2050 日期，超过 MySQL TIMESTAMP 可写范围而失败，已修正为 2020–2023；没有改数据库错误行为。新增 legacy 夹具初版 bigInteger 误传额外参数，预编译后改为现有 API。首次 Playwright 未找到要求的 Chromium 1254，显式使用既有 Chromium 1243（Chrome for Testing 153.0.8010.12）完成检查；不声称默认浏览器下载已验证。Node 24.20.0、Playwright 1.65.0。

## 未闭合项与下一批

本批不是完整 Client 冻结，也不是 Passport-02 整单元完成。仍需：

1. Client 未绑定实例 query／save 的 passport.connection 回退、完整关系、工厂、plain_secret 显式追加、hasScope／继承 scopes 与 UUID 开关机制。
2. delete 按实际 Token／RefreshToken 模型生命周期撤销，再撤销 Client；不以裸批量更新跳过事件。
3. 完整 passport:client 的选项、提示、输出和真实 Artisan 创建／错误路径；本批只验证原有 Artisan 不回归。
4. 固定 PHP oracle 的数组／比较／秘密失败顺序、浮点字符串转换，以及 Scope JSON options／Unicode／斜杠字节规则。当前 Scope 测试只验证语义 JSON，不宣称字节等价。
5. 完整 HTTP 管理／跨用户拒绝等整单元验收。当前真实 HTTP覆盖仓储查询、请求隔离与双应用，不冒充协议端点或完整管理 API。

发布前其他驱动、Windows、完整安全审计和 rsa 公告继续保留原账本。V06 维持部分验收，核心 66/66 不变；不推进 Passport-03。下一批仍是 Passport-02 的模型连接／关系及撤销前置，再接客户端命令。

## 最小提交范围

```text
Cargo.lock
vendor/openrust-framework/src/
├── database/eloquent/model.rs
└── support/{mod,value_comparison}.rs
vendor/openrust-passport/
├── Cargo.toml
├── src/{lib,passport,client,client_record,client_repository,php_array,scope}.rs
└── tests/
    ├── client.rs
    ├── client_lookup.rs
    └── support/client_example.rs
docs/{en,zh_CN,zh_TW}/packages/passport.md
.agents/project/
├── current.md
├── progress.md
└── audits/
    ├── core-closure-ledger.md
    └── passport-02/{README.md,source-sha256.json}
```

资源收尾和最终提交范围见下方最终记录。本轮沿用最新真人取消人为额度停线授权；原 Goal 已由用户恢复并核实 active。正式额度工具不可用，不冒充旧读数、不启用 credits；50 GB 磁盘停线保持，禁止推送。

## 最终资源与提交记录

24 个不同测试函数全部通过；18 个核定源码／测试／正文文件与独立快照 SHA 一致，34 个应用基线文件不变。本批加三份交接／账本和两份专项记录，共 23 文件。所属提交可用 `git log -1 -- .agents/project/audits/passport-02/README.md` 定位，提交后实际哈希与文件清单另存本地验收目录 commit.json。

确认所有自有验收进程退出后，直接删除本批 source／target／浏览器依赖，逻辑 5,202,987,593 字节，df 实际可用增加 5,010,182,144 字节，清理后可用 80,071,938,048 字节。共享 root target、Cargo registry、docs/node_modules、现有 Chromium 和所有 WIP 保留；共享根文档 dist 已由其他工作移除，本批没有再次删除或归为自身释放。保留原始日志、18 文件 overlay 和脚本供追溯。

远程标准夹具全部退出，无验收容器遗留。核对批前仅有共享 Rust 镜像、临时 MySQL 镜像精确 SHA 且无容器引用后回收；共享 Rust 镜像及数据卷清单不变，删除本批专属远程包装目录。未用 prune 或删除数据卷。远程 sudo 的主机名解析提示保留在日志，命令与资源核对成功，没有改动远程系统配置。

## 模型连接回退接续

2026-11-19 [模型连接回退](connection-fallback.md)已通过直接模型、事务、真实 HTTP／MySQL、Artisan 与三语验收。上方历史未闭合清单第 1 项中的未绑定连接回退已处理，关系／工厂等其余部分继续推进。Passport-02 整单元仍未闭合。

## 客户端 scope 与显式追加接续

2026-11-19 [scope 与显式明文追加](scopes-and-appends.md)通过固定 PHP 312 例、29 个 Rust 测试函数、真实 HTTP／MySQL、Artisan Kernel 与三语验收。历史清单中的 scope 继承和 plain_secret 显式追加已处理，完整 Client／Passport-02 仍未闭合。

## 客户端命令与撤销接续

2026-11-20 [客户端命令、关系与撤销](client-management.md)通过 44 个测试函数、实际 MySQL／PTY／HTTP／Artisan 与三语验收。历史待办中的 UUID 开关、编译关系、令牌撤销及 passport:client 已处理；剩余固定 PHP oracle 继续单独核验。

## 2026-11-20 单元验收结论

[PHP 契约与 HTTP 管理验收](contracts.md)补齐上方历史未闭合清单：Scope JSON、secret setter 失败顺序／浮点转换、固定 PHP 对照与实际认证管理流程。结合连接、scope／追加、工厂、命令／关系／UUID／撤销链各批证据，Passport-02 的计划范围已完成验收；V06 保持部分验收，后续 Passport-03 至 09 按计划推进。历史首批清单保留为当时停点。完整 PHP ORM／警告通道、Windows 和发布安全矩阵不在本批结论内。
