# 数据库翻译扩展验收

2026-11-21。隔离基线 `db854be5b`；本批只实现 `openrust-translation-database`、本包测试与三语 localization 正文。不修改共享框架／API 站点／公共导航；认证 PostgreSQL 完整联测独立记录于 [init-auth 补验](../00-03/init-auth.md#2026-11-21-postgresql-18-完整认证补验)，不纳入此处通过结论。

## 上游调研与复用

联网读取官方 GitHub API、固定源码、许可证和测试，不以星数作为设计依据：

- [Spatie Laravel Translation Loader 2.8.3](https://github.com/spatie/laravel-translation-loader/tree/17fa7d211bca1423a0e424823e7fddd5c2bc4ab6)，提交 `17fa7d211bca1423a0e424823e7fddd5c2bc4ab6`，MIT。核对 README、LanguageLine、TranslationLoaders/Db、TranslationLoaderManager、migration、Feature/TransTest、JsonTransTest、LanguageLineTest、TranslationManagerTest 与 DbTest。采用 `language_lines` 的 `group`／`key`／`text` 多语言 JSON 结构、group `*` 的字面键、普通组点分键、数据库覆盖文件、包命名空间只走文件的行为。保留上游 MIT 许可于包内 `LICENSE.spatie.md`。
- [Barryvdh Laravel Translation Manager](https://github.com/barryvdh/laravel-translation-manager/tree/60b0ebdbf997dd55346cf6219d321876fb6eb03c)，提交 `60b0ebdbf997dd55346cf6219d321876fb6eb03c`。README 明确是文件导入／数据库编辑／导出管理，不直接替换运行时翻译系统，因此不引入管理 UI、扫描与导出工具。
- [Spatie Laravel Translatable](https://github.com/spatie/laravel-translatable/tree/bc0a5ffba85fcd207320e095dc0f23566d56d734)，提交 `bc0a5ffba85fcd207320e095dc0f23566d56d734`。用于业务模型字段多语言，职责不同，不将其当作界面词条 Loader。
- 固定 Laravel `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 Loader／FileLoader／Translator 契约见 [03-10](../03-10/README.md#可插拔异步翻译源)。本包复用现有异步 Loader 与 `Lang::loadFrom`／`Translator::loadFrom`，不替换框架的查找系统。
- 核对旧版 `v0.x` `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 的包源码、README 与测试。保留精确语言来源合并、空字符串覆盖、失败保留已发布目录和应用隔离用例；不迁入旧 MVCC 修订、Mutation、事件、通知与后台刷新机制。现有 FileLoader 的递归组／列表索引合并规则在本包复用。

PHP 依赖不能直接链接 Rust；本包只增加对现有 framework、serde_json 的生产依赖，测试使用已有 tokio／tempfile／axum，锁文件不引入新 registry 版本。`DatabaseLoader` 保留主线已定公开类型名；显式 Connection 代替 PHP 全局模型／provider 解析，写入与迁移使用现有 Query Builder／Schema，无需增加新的仓储或管理框架。

## Rust 适配与支持边界

- 同一行 `text` 保存各精确规范语言的字符串；null／缺失保留文件值。语言回退由现有 Translator 统一执行，而非 Spatie 模型每行先回退，防止回退语言数据库词条覆盖所选语言文件。
- 加载异步、翻译查找同步，显式物化后一次发布；没有第二套缓存或自动刷新。
- 缺表、数据库／文件／解析故障均返回 Source，保留旧目录。与 Spatie 缺表自动回文件的行为不同，避免显式重载失败时丢失已发布数据库词条。
- 每组默认 10,000 行，上限加一检测超额且不截断；重复／父子冲突与超过 128 层点分键报错，避免数据库行顺序决定结果。SQL 参数绑定；再次精确比较 group 防止排序规则混入资源。
- 显式选择表和连接，支持连接自身前缀；包命名空间不查询数据库。发布原子性不承诺跨组 SQL 事务快照。
- 数据库行为最初只验证 PostgreSQL 18；后续 MySQL 8.4、SQLite 实测见下方补验。不新增翻译管理后台、安装命令或隐式迁移。

## 实际验证

隔离源码与证据：`/private/tmp/openrust-translation-database-20261121/final-source/`；专属 target 位于同批 `target/`。Rust 1.94.0，`CARGO_INCREMENTAL=0`，jobs=3，dev/test debug=0。未在活跃工作树构建，未连接开发数据库。

- `python3 -B tests/postgres_acceptance.py -- cargo +1.94.0 test --locked --offline -p openrust-translation-database`：PostgreSQL **180006**，8 项测试全部通过，4 个 rustdoc／README 示例全部通过。覆盖 native JSON 正式表、默认表名、多语言／null／点分与字面键、文件／列表索引／空串／回退与删除恢复、重复冲突／格式错误／行数与深度限制、批次失败不发布、独立连接／表／译器，以及两个真实 HTTP 应用 Provider 装配和 Lang 门面隔离；包命名空间无数据库表时仍能读取文件。
- `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` 与 `cargo +1.94.0 fmt --all --check` 通过。
- `node docs/tests/check_content.mjs`、三语 `npm --prefix docs run build`、`check_browser.mjs` 的根／子目录三语桌面与移动 12 项、`check_translation_database.mjs` 的两挂载三语 6 项全部通过。
- 初次正式表测试发现 Schema::string 要显式传 None 长度，修正实现夹具与所有使用示例后重跑；早期五字段方案仅保留隔离候选，未合入。

完整日志保留同批目录中的 `final-postgres-complete.log`、`final-clippy-complete.log`、`final-fmt-complete.log`、`final-docs-content.log`、`final-docs-build-verified.log`、`final-docs-browser.log` 与 `final-docs-translation-browser.log`。API 页面和快照由核心框架对话更新。

验收后清理自有闲置 target，保留源文件与日志；文档依赖为符号链接，只解除本批链接，未删除共享依赖。随后清理自有文档构建产物，第二遍 Available 从 56,660,884 KiB 到 56,786,064 KiB，实测差额 125,180 KiB；首遍 target 清理遇到符号链接保护后停止，未记录完整首遍前后差值。保留所有源码、失败夹具、固定上游源码、日志、数据库及数据卷。清单见同批 `cleanup-final.json`。


## 2026-11-21 三库补验

基于 `95e904727` archive，隔离源码 `/private/tmp/openrust-init-auth-20261121/source`，复用已无原构建进程的本批专属 target；Rust 1.94.0，2 jobs，增量关闭、dev debug=0。测试通过 `OPENRUST_TEST_TRANSLATION_DRIVER=pgsql|mysql|sqlite` 运行同一组行为，PostgreSQL／MySQL 强制要求隔离 wrapper 的 URL、REQUIRE 与容器标识，SQLite 创建独立临时文件，不访问应用数据库。

- PostgreSQL 18.6（180006）、MySQL 8.4.13、SQLite 各 **8 项行为测试 + 4 个 rustdoc／README 示例**全部通过。每库均覆盖多语言及点分／字面键、空串／null／文件覆盖／列表与语言回退、错误目录与限制的原子发布、重复冲突、独立连接和应用、Provider 真实 HTTP、原生 JSON 与默认 `language_lines` 表结构。
- 首轮 SQLite 夹具未显式创建文件而连接失败；按框架现有 `create_if_missing(false)` 契约创建专属临时文件后通过，没有放宽生产连接行为。
- 生产翻译 Loader 无需新增抽象、仓储、缓存或驱动特例；复用现有 Connection／Query Builder／Schema 即可完成三库。Spatie 2.8.3 `TranslationLoaders/Db.php` 固定源码再次联网核对，沿用前述契约。

证据在同批 `translation-postgres.log`、`translation-mysql.log`、`translation-sqlite.log`。文档中的支持范围同步更新为 PostgreSQL 18、MySQL 8.4 和 SQLite；其他数据库仍需独立行为验证。
