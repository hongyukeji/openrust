# 05-03 forceFill 保留属性转换与哈希

状态：实现和本批实际验收通过。Passport-02 的必要公共前置修复，不把此补丁算成 Passport-02 客户端／仓储已交付。

## 固定依据和复用

- Laravel Framework 13.40.0 SHA `1449bf241017087fbd38c118ecaabef3f6797fdc`：[Model::forceFill](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php#L750) 调用 unguarded → fill，仍走属性修改器和转换器；[HasAttributes](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Concerns/HasAttributes.php) 的 hashed 字符串配置校验保持现有实现。对应固定文档 `eloquent-mutators.md#hashed-casting` 和 `eloquent.md#mass-assignment`。
- 固定 Passport 13.10.0 SHA `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 Client::secret、ClientRepository::update／regenerateSecret 依赖赋值后哈希，不能沿用主线原始属性覆盖行为。
- `../openrust-v0.x` HEAD `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`：检查 `framework/src/database/eloquent/mass_assignment.rs`、`mass_assignment/tests.rs` 与 `save/tests.rs`，提取 force_fill 通过赋值流程、错误后恢复、pending／取消时任务隔离的规则和回归。旧版 Passport `client.rs` 的临时明文／hashed cast 和 `repository.rs` 的只读边界已核对；不复制旧数据库／容器接口。本批不改固定 Goravel 1.18 的入口或 Runner。

## 问题与实现

原 Model::forceFill 直接 apply_attributes，会把明文或未转换值写入模型，并绕过修改器。现在只在当前调用内跳过批量赋值保护，然后调用模型自己的 fill；返回 DatabaseResult<&mut Self> 传播转换失败。两处原调用已迁移：默认关系模型向上传错，现有模型回归显式 unwrap。

增加 forceFillAsync，通过已有 Tokio task-local unguarded 执行模型自己的 fillAsync；明文哈希仍使用所属应用 Hash 服务的有界线程池，不阻塞异步执行器。同步 forceFill 接受空值／已通过配置校验的哈希，遇到需计算的明文返回错误，不会存原文。默认批量赋值保留既有全部成功后安装属性的机制，错误保留原属性；自定义 fill／fillAsync 覆写保持自身职责。

Rust 适配：同步版本需要 Result，异步哈希需要 .await；没有进程全局 guard 开关，没有 Passport 专用哈希绕行。公开 forceFill 保留 Laravel 名称，forceFillAsync 与当前 fillAsync／setAttributeValueAsync 的 Rust 异步边界一致。新的 bypass 只影响显式调用，不改变普通 fill 的保护或 Query Builder 原始更新语义。

## 实际验收

macOS ARM64、Rust 1.94.0；真实 MySQL 8.4.13 在专属 tmpfs Docker 实例上，以 SSH 回环端口转发提供给原生 Mac 测试。用项目 `tests/mysql_acceptance.py` 管理生命周期，严格设置 REQUIRE 开关，临时凭据仅在控制进程内传递、不写日志。没有运行 SQLite 或其他数据库测试；未新增依赖。

```sh
cargo +1.94.0 test --locked -p openrust-framework --test eloquent_force_fill
cargo +1.94.0 test --locked -p openrust-framework --test eloquent_hashed_casts hashed_model_casts_on_mysql_8_4 -- --exact
cargo +1.94.0 test --locked -p openrust-framework --test eloquent_models model_persistence_and_scopes_on_mysql_8_4 -- --exact
cargo +1.94.0 test --locked -p openrust-framework --test eloquent_relationships relationship_keys_and_query_counts_on_mysql -- --exact
cargo +1.94.0 test --locked --test console --test feature --test artisan_entry
cargo +1.94.0 build --locked --bin main
./artisan inspire
cargo +1.94.0 clippy --workspace --all-targets --locked --features pail,passport --offline -- -D warnings
cargo +1.94.0 fmt --all --check
npm --prefix docs run build
```

4 项专项测试覆盖同步／异步自定义 fill 覆写、修改器／JSON、转换失败不污染属性、明文拒绝／异步哈希、已有哈希／null、配置不符拒绝、普通／并行任务的保护恢复，以及借鉴旧版的嵌套 bypass、pending poll 与取消清理。真实 HTTP→Kernel→Eloquent→MySQL 连续两次成功与两次 JSON 500 后恢复；默认隐藏 secret／password，数据库重读验证两个字段都为可验证哈希。三语完全相同的 Rust 文档函数由 fixture 编译并在实际 HTTP 保存路径执行。

共 **30 个不同测试函数通过**：最终 4 项专项、真实 MySQL 的 hashed casts／models／relationships 三项，以及根 Console／Feature／Artisan 23 项。关系回归实跑 525.79 秒后终态通过，未把 live／超时观察计为通过。专项最终日志中的泛用“HTTP／Artisan acceptance PASS”输出只覆盖该次选择的四项；Artisan 证据必须来自完整控制日志中的独立命令，而非该汇总标签。

根入口首轮 6/7 Console 通过，Facade HTTP 为 500；临时诊断明确为 Configuration("compiled Eloquent model")。工作区 `storage/framework/config.json` 创建于 2026-11-18，早于本批，测试应用误加载该缓存而没有根应用的模型注册。保留原文件，显式将 APP_CONFIG_CACHE 指向本批不存在的独立路径（并使用 array Session／Cache）后，完整根 23 项通过。缓存 SHA／mtime 未变，临时诊断测试已移出工作区留作证据。未改产品代码掩盖缓存／配置失败。

实际 `./artisan inspire` 分别在默认 Rust **1.99.1** 和 `RUSTUP_TOOLCHAIN=1.94.0` 退出 0。默认工具链的 Debug 链接出现 `__eh_frame section too large (max 16MB)` unwind 表警告，最低工具链严格 Clippy／格式及实际 Artisan 均通过；不将 Debug 警告等同于发布目标矩阵通过，也不扩大修改构建 flags。

首次新测试缺少 Application trait 导入，后续修正；主键 u64 改为当前 Value 支持的 i64；JSON 默认原始存储使用 Value::Json，移除误假定为 Text 的测试断言，未改产品 JSON 行为。最终编译／测试记录与这些失败分开。

9 份源码／测试／文档 SHA 已保存，三语 Rust 示例与实际执行 fixture 逐字节一致；56 份入口／bootstrap／应用配置／路由／Cargo／Passport 源码基线不变。三语 mutators 的 Hashed Casting 小节同步更新，12 项通用浏览器与 6 个三语×部署前缀章节检查覆盖 `/` 和 `/docs/`。未新增 Passport 客户端命令或占位 HTTP 管理接口。Windows 和跨数据库仍按发布矩阵核定。

## 最小范围

```text
vendor/openrust-framework/
├── src/database/eloquent/
│   ├── model.rs
│   ├── concerns/guards_attributes.rs
│   └── relations/default_model.rs
└── tests/
    ├── eloquent_models.rs
    ├── eloquent_force_fill.rs
    └── fixtures/force_fill.rs
docs/{en,zh_CN,zh_TW}/eloquent/mutators.md
.agents/project/
├── audits/05-03/force-fill.md
├── audits/core-closure-ledger.md
├── current.md
└── progress.md
```

13 文件；Cargo 清单／锁、main／bootstrap／应用配置／routes 和 Passport-01 源码不变。保留所有既有未跟踪补丁、core/、translation-database/。上一单元 Passport-01 已提交 `9f086981252fde12c756afb54fd6e9c71cd9b7a8`，不是遗留未提交文件。

## 收尾

确认所有 Cargo／Rustc／验收容器退出后，直接删除本批 root target（含 1.94／1.99 双工具链）、站点和临时浏览器依赖，逻辑字节 **25,170,181,323**，df 实际可用增量 **21,908,541,440** 字节；两种数字分开记录。清理后本地约 **82.45 GB**，高于 50 GB 停线。保留共享 Cargo registry、docs/node_modules、现存应用缓存、源码及全部 WIP。

远程 MySQL 专属实例均由项目包装器回收，无残留容器；核对原始清单、精确镜像 SHA、无引用后直接删除本批新增 mysql:8.4 镜像与 24 KiB 包装目录，保留共享 Rust 镜像和全部数据卷（前后一致）。远程 Available **105,895,196 KiB**。原始日志、终态记录、工具链／保护缓存证据、SHA、固定上游参考和下一单元契约矩阵保留在 `/private/tmp/openrust-passport-02-20261119`。

协作者新增的 `.agents/project/{plan,context}.md` install:full 计划保留，不混入这次基础修复提交。仅本地提交，不推送、不改发布版本；现有 Goal 保持 active。最新真人取消人为额度停线，正式额度读取工具仍不可用，不冒充实时读数，不启用 credits；50 GB 磁盘停线保留。继续 Passport-02 的 Client／ClientRepository／passport:client，UUID 查询门禁、哈希和临时明文、类型化模型生命周期／request-once 身份缓存及跨应用隔离须保留，不能直接用旧版只读仓储冒充。
