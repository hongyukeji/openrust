# Passport-02 模型连接回退

2026-11-19，固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 Client::getConnectionName 与 Laravel 13.40.0 Eloquent 查询、保存及事务生命周期。基线为 `c2bfdd73f01060b774c52b72333eb834e581dae3`，沿用[首批模型与仓储](README.md)及[交接归属](handoff.md)。旧版客户端与仓储已只读核对，复用同应用配置与绑定连接优先的原则，未复制旧模型快照实现。

## 实现范围

- Eloquent Model 新增 `resolveConnectionName` 和 `getConnection`。前者返回 `DatabaseResult<Option<Cow<'_, str>>>`，允许模型持有从应用配置读取的动态名称，保留既有 `getConnectionName` 的借用签名。后者优先返回模型已绑定的真实 Connection。`query` 从模型实例取连接；持久化复用同一入口。
- Client 在无显式连接时读取当前应用的 `passport.connection`，然后回退默认连接。仓储和模型复用原有配置值转换，不新增依赖或改动 Cargo 清单。浮点字符串精确转换仍归 PHP oracle 后续验收。
- 未绑定模型的事务写入比较动态连接名；已绑定模型继续校验真实 Connection 的事务归属。数据库错误正常传播。更改配置不会重新绑定已加载模型。
- 本批未补关系、工厂、撤销链、scope 继承、明文追加或客户端命令。Passport-02 与 V06 继续部分验收，Passport-03 前置尚未满足。

## 验收

使用 `git archive c2bfdd73f` 加本批 5 份源码／测试和 3 份正文建立隔离源码。未叠加共享工作区的 init、Session、Encryption、SMTP、ViewData 改动。本批 8 文件与隔离副本 SHA 一致，见[源码清单](connection-source-sha256.json)。专属目录 `/private/tmp/openrust-passport-02-connection-20261119/` 保留日志和执行脚本。

| 检查 | 实际结果 |
| --- | --- |
| 原生 Client／数组／Scope | 6 项通过 |
| 既有 Passport 生命周期 | 10 项通过 |
| MySQL client_lookup | 9 项全部通过，0 忽略，MySQL 8.4.13 |
| 新增真实 TCP HTTP | 直接 query／save／find／delete、默认与显式连接、正确事务回滚、错误事务拒绝、更改配置后保持绑定、错误配置传播通过 |
| 核心 Eloquent | MySQL 专项 `model_persistence_and_scopes_on_mysql_8_4` 通过，未运行混合 SQLite 用例 |
| 实际 Artisan 与根 HTTP | 编译启用 Passport，inspire／passport:keys 帮助及原始 ./artisan inspire 通过；首页与 /up 200，Web／API／Accept JSON 404 通过；SIGINT 与端口释放通过 |
| Rust 1.94.0 | 隔离全仓 fmt、features pail,passport 全工作区全目标严格 Clippy 通过 |
| 三语 | 构建、12 项通用与 6 项 Passport 浏览器检查通过，覆盖根及 /docs/、三语、移动与桌面；既有 Vite 大 chunk 提示保留 |

实际命令与回执入口如下。

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 clippy --workspace --all-targets --offline --locked --features pail,passport -- -D warnings
cargo +1.94.0 test --offline --locked -p openrust-passport --test client --test lifecycle
python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --offline --locked -p openrust-passport --test client_lookup -- --test-threads=1
```

核心 MySQL 用例通过本批 `core_mysql.py` 将夹具 URL 显式传给 `OPENRUST_TEST_ELOQUENT_MYSQL_URL`，同时设置必需执行开关。各次实例均由项目 mysql_acceptance.py 启动，使用 tmpfs 数据目录和独立标签，完成后回收容器，保留全部数据卷。

首次新增 HTTP 夹具返回 500，数据库写入失败。补齐回滚事务样本漏填的非空 `redirect_uris` 与 `grant_types` 后，全 9 项通过；保留 mysql.log、mysql-diagnostic.log 和 mysql-final.log。没有放宽断言或修改迁移默认值。系统 PHP 7.4 因缺 libtidy 无法启动，本批未宣称完成固定 PHP oracle。

## 后续

继续 Passport-02 完整关系／工厂、scope／UUID 开关、plain_secret 追加、Token／RefreshToken 生命周期撤销、完整 passport:client、固定 PHP oracle 和完整管理接口验收。整个 Passport 扩展尚未闭合。

## 资源收尾

已确认本批没有运行中的 Cargo／Rust／Node／HTTP 进程，四次 MySQL 夹具标签均无残留容器。保存 accepted-overlay.tgz、SHA 清单和成功／失败日志后，直接删除专属 source、target 与 browser 依赖，逻辑 4.914 GB，df 观测增加 4.730 GB。清理后可用 71.47 GB（66.56 GiB）。共享工作区并行构建可影响 df，总量以该时点观测为准；共享缓存、其他 WIP、数据库及数据卷保留。回执为同目录 cleanup.json。本批按核定范围本地提交，不推送。
