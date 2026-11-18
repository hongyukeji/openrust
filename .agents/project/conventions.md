# 项目规范

## 上游参考

- 以[固定 Laravel 基线](plans/laravel-rebuild-baseline.md)确定目录、公开契约、默认值和可观察行为。
- Goravel 固定参考 v1.18.0。框架使用 `a554ced468cada50a269780692a0491e5f9e3a11`，应用骨架使用 `31651f646ccedf1701471bf01285111270a9d370`。参考其应用入口、装配和编译型运行方式，并验证 Rust 实现与 Laravel 的等价行为。 [Goravel 框架源码](https://github.com/goravel/framework/tree/a554ced468cada50a269780692a0491e5f9e3a11) · [Goravel 应用骨架](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## Laravel 到 Rust 的移植原则

- 把固定 Laravel 基线当作从 PHP 翻译到 Rust 的原文。像多语言翻译一样，保留原意和结构，用 Rust 表达相同的机制。默认装配、生命周期顺序、默认值和用户可观察行为均按上游保留，让熟悉 Laravel 的开发者可以逐项对照 PHP 源码阅读 Rust 实现。
- 对应的公开方法名、类型名、参数名、配置键、环境变量名和内部变量名，凡 Rust 能直接表达，尽量保留 Laravel 的原名和大小写。例如 `$basePath` 对应 `basePath`，PHP 命名参数 `web:` 对应 Rust Builder 的 `.web(...)`，`APP_BASE_PATH` 保持原样。
- PHP 的命名参数、动态加载、异常等按需转换为 Rust 的 Builder、静态注册、`Result` 和异步接口，保留原有职责和调用时机。对应方法保留 Laravel 的拼写和大小写，例如 `withRouting` 与 `withExceptions`。
- Rust 机制能带来更好的性能时，可以替换内部实现，前提是保持 Laravel 的核心机制、公开契约、默认值和错误行为。以针对性的测试验证等价行为，以测量支持性能收益。对每项语言适配或性能取舍，记录对应的 Laravel 源码、采用的 Rust 实现、原因和验证结果，并在交付时告知用户。

## 骨架与开发顺序

1. **骨架按正式版设计。** 编码前按[固定 Laravel 基线](plans/laravel-rebuild-baseline.md)确定应用、框架和扩展的边界，明确目录、文件路径、模块、公开导入路径、类型名、配置键、命令和三语文档入口。已确认的结构可先预留，尚未确定的契约列为待定。
2. **命名跟随上游。** 公开接口及对应的内部变量按[移植原则](#laravel-到-rust-的移植原则)命名。上游名称中含 Laravel 品牌段时，将品牌段替换为 OpenRust。Rust 的文件和模块按语言惯例组织。因语言或运行时限制调整接口时，记录对应关系、具体约束和等价行为的验收方式。可参考固定 Goravel 版本的跨语言实现。
3. **Artisan 先定契约。** 逐项核对命令名、命名空间、公开别名、参数及顺序、选项、默认值、帮助、输出、退出码和副作用。目录和说明可以先行，未实现的命令标记为占位，执行时明确失败。相同命令合并，行为以 Laravel 为准，Goravel 独有命令单独标明来源。机器可读输出也按契约核对。
4. **按依赖逐簇填充。** 每一簇先核对固定源码、文档和适用的上游测试，再填实现、独立测试与对应文档。验收覆盖实际入口、默认行为和关键失败路径。骨架定稿、实现完成和验收通过分别记录。

## 代码与测试目录

- 应用代码放在根应用的对应目录，框架源码放在 `vendor/openrust/framework/`。
- 测试代码单独存放。根应用测试放在根目录的 `tests/`，crate 测试放在所属 crate 的 `tests/`。

## 旧版实现复用

1. **先定契约，再查旧版。** 实现前优先查阅同级 `openrust-v0.x` 工作树的 `v0.x` 分支。没有该工作树时，在当前仓库用 `git show v0.x:<仓库内路径>` 查阅。没有本地分支时，可用已有的 `git show origin/v0.x:<仓库内路径>`。两种方式均无需切换分支。记录对应源码、测试和修复的提交与路径，来源不可用时明确记录。
2. **按职责提取。** 可直接复用符合契约的内部实现、驱动适配和回归用例。旧版公开接口或目录与当前契约不一致时，提取有用的实现并接入当前模块。涉及依赖或跨组件调用时，核对许可证、Rust 版本、生命周期和安全边界。
3. **新主线按当前契约交付。** 每批只引入对应功能所需的代码和测试。旧 API、配置键、数据格式及迁移历史不作为兼容目标。与固定基线冲突的行为按当前契约修正，旧版缺口按依赖顺序补齐。
4. **用当前入口重新验收。** 复用旧测试时同步更新路径、接口和断言。以固定上游用例、当前应用入口及失败路径验证本批行为，分别记录复用范围、差异和实际验收结果。

## 文档规范

1. **章节对齐上游。** 按[固定 Laravel 文档基线](plans/laravel-rebuild-baseline.md#文档导航核对)组织章节和阅读顺序。英文、简体中文、繁體中文使用相同的章节相对路径和顺序。OpenRust 独有能力在导航中标为「扩展」。
2. **同一内容只维护一处。** 一个主题只有一个正文页面。其他章节用相对 Markdown 链接引用，确保在 GitHub 和部署到子目录的 VitePress 站点都能跳转。上游多个入口指向同一主题时，保留对应导航入口，共用正文。
3. **功能验收后同步正文。** 按 Laravel 对应章节介绍默认行为、配置和用法。Rust 示例与 Artisan 命令须实际验证。因 Rust 语言或运行时产生的差异须说明原因，并以测试验证等价行为。尚未验收的功能只保留空白占位。
4. **三语一起校验。** 同步更新三个语言版本的正文和导航，统一术语、标题层级与章节顺序。检查章节路径和内部链接，运行文档构建及现有浏览器验收。站点命令见[文档 README](../../docs/README.md)。

## 配置文件注释

- **配置组。** 在每组上方使用 Laravel 风格的简体中文 `/* … */` 块注释，标题上下加横线，标题和正文各行以 `|` 起首。说明用途和对应环境变量，按需写明默认值、回退行为或覆盖顺序。调整配置时同步更新注释。 [应用配置](../../config/app.rs)
- **辅助方法。** 优先级、回退或解析规则不明显时，在方法声明正上方使用多行简体中文 `/* … */` 块注释。正文每行以 `*` 起首，不加分隔线。先说明用途，再交代适用条件、处理顺序和必要的错误行为。 [环境变量读取](../../config/env.rs)

## 验收清单

- **Rust 代码。** 在仓库根目录使用最低支持工具链 Rust 1.94.0 运行 `cargo +1.94.0 fmt --all --check` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings`。按改动验证实际入口、默认行为和关键失败路径。骨架改动验证公开导入和签名编译，分别记录骨架、实现与行为验收状态。
- **测试范围。** 日常执行与改动相关的定向测试。数据库用例只运行 MySQL 8.4，筛选和连接验证见下文。SQLite、其他数据库及最低支持版本统一在发布前按 Laravel 兼容基线验收。
- **文档。** 纯指引改动检查引用、路径和锚点。文档站改动按[文档 CI](../../.github/workflows/docs.yml)构建三语站点，并运行浏览器检查，覆盖 `/` 与 `/docs/`。
- **记录。** 记录实际执行的命令、环境和结果，说明跳过项或外部服务限制。检查本批差异及空白错误，暂存、提交和推送遵循当次授权。
- **验收收尾。** 每批验收保存记录后，确认产物归属本批、无人占用且可重建，再直接删除，跳过系统废纸篓或回收站。复查磁盘并记录实际释放和剩余空间，再开始下一批。保留源码、未提交改动、验收记录、数据库及数据卷、共享缓存和发布产物。清理容器时，`docker container rm` 和 `docker compose down` 均省略 `-v`／`--volumes`。操作前后核对数据卷，匿名数据库卷同样保留。

### 日常 MySQL 8.4 验收

总控临时启动 MySQL 8.4 时，统一用 `python3 -B tests/mysql_acceptance.py -- <验收命令>`。本地 Docker 须已有 `mysql:8.4` 镜像。入口核对镜像声明的数据目录，以 1 GiB 的 `tmpfs` 承载数据，使用随机回环端口、临时凭据和当次专属标签。实际查询版本成功后，向验收命令提供 `OPENRUST_TEST_MYSQL_URL`、`OPENRUST_REQUIRE_MYSQL=1` 和 `OPENRUST_ACCEPTANCE_MYSQL_CONTAINER`。同批涉及多个 schema 时在该隔离实例上创建。

入口在命令完成、失败或收到可捕获的取消信号后回收该标签的容器。若脚本被强制杀死或 Docker 不可用，按启动日志中的批次标签复核容器归属后收尾。表目录和 schema 范围复验运行 `sh tests/table_catalog.sh`。独立源码快照的 Cargo 构建产物须按快照隔离，应用生成的模块路径引用当前快照。夹具生命周期回归运行 `python3 -B tests/mysql_acceptance_lifecycle.py -v`。

从仓库根目录执行。使用独立测试库，测试会建表、删表和写入数据。先用测试账号连接与测试 URL 相同的主机、端口及库名，执行 `SELECT VERSION()`，确认实际连接的是 MySQL 8.4.x，再运行测试。

```sh
mysql --protocol=TCP -h 127.0.0.1 -P 3306 -u acceptance -p openrust_acceptance \
  --batch --skip-column-names -e 'SELECT VERSION()'

export OPENRUST_TEST_MYSQL_URL='mysql://acceptance:REPLACE_ME@127.0.0.1:3306/openrust_acceptance'
export OPENRUST_REQUIRE_MYSQL=1
cargo +1.94.0 test --locked -p openrust-framework --test database_query \
  query_builder_runs_on_mysql_8_4 -- --exact
```

若使用 Docker 中的独立 MySQL 测试实例且宿主机没有 `mysql` 客户端，可用容器内的客户端验证版本。下面以 `openrust-mysql` 为容器名，先用 `docker port openrust-mysql 3306/tcp` 确认宿主机测试 URL 使用的是该容器的映射端口。

```sh
docker exec -it openrust-mysql mysql --protocol=TCP -h 127.0.0.1 -P 3306 \
  -u acceptance -p openrust_acceptance --batch --skip-column-names -e 'SELECT VERSION()'
```

替换示例账号、密码和库名，URL 中的特殊字符按 URL 规则编码。不同测试读取各自的环境变量。按测试源码中的 `std::env::var` 和 `OPENRUST_REQUIRE_*` 设置。对于读取 URL 失败会跳过的测试，同时设置对应的 `OPENRUST_REQUIRE_*=1`。例如关系批量写入测试使用同一测试库时执行：

```sh
OPENRUST_TEST_RELATION_MYSQL_URL="$OPENRUST_TEST_MYSQL_URL" \
OPENRUST_REQUIRE_RELATION_MYSQL=1 \
cargo +1.94.0 test --locked -p openrust-framework --test eloquent_relationships \
  related_upsert_on_mysql -- --exact
```

可先执行 `cargo +1.94.0 test --locked -p openrust-framework --test eloquent_relationships -- --list` 核对完整测试名。用 `-p`、`--test` 和 `--exact` 定位到指定测试，并确认输出实际执行且通过了预期用例。日常验收避开无筛选的 `cargo test --workspace --locked`，该命令会运行工作区的 SQLite 等数据库测试。

`--exact` 只筛选测试函数。执行前核对函数及其辅助夹具实际使用的驱动。`relationship_keys_and_query_counts_on_mysql` 的连接隔离夹具也使用独立的 MySQL 连接。遇到其他在单个函数内混跑多数据库的用例，先拆出独立的 MySQL 用例，或让夹具随本用例选择驱动，再精确执行。仍混用多数据库的用例统一留到发布前验收。

## Git 提交标题

所有代理统一使用 Conventional Commits 格式。类型和适用的作用域使用英文，冒号和空格后的变更说明使用简洁中文。带作用域的标题格式为 `<type>(<scope>): <中文说明>`。

## 持续开发交接

本轮资源阈值、阶段缓存清理及 Git 授权见[持续开发交接记录](current.md)。
