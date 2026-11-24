# 生成与安装产物的分层回归

2026-11-24。承接默认骨架、make:model、Sanctum／Passport／Fortify 安装发布模板的静态核对，按[分层规范](../../conventions.md#模型数据库访问与业务服务分层)增加生成产物回归。只修改测试及记录，不调整安装器、业务实现或用户应用；复用既有 SourceInstallation、MySQL 隔离入口、生成应用编译及真实 HTTP／Artisan 验收方式。

## 测试边界

- `tests/generated_model_layering_contract.py` 对当前默认及生成模型检查具体存储／业务编排入口；对 User 的安装前后 token 差异采用白名单：Sanctum 空 trait、Passport Provider 名称、Fortify 空 trait／Clone／隐藏字段。安装器新增未声明的方法、查询或业务流程会导致差异断言失败；不是全局禁止模型 query/save 或应用定制的通用 lint。
- `tests/generated_model_layering_contract_test.py` 验证断言能拒绝注入存储／业务代码、删除或改写旧代码、重复声明及越界能力；同时允许保留已有数据库定制，检查普通／raw 字符串与注释不混淆。测试工具不自动改写或重构用户模型。
- `tests/generated_model_layering.py` 只允许在项目 target/tmp 下隔离源码中运行，复制实际默认骨架，在独占 MySQL 中调用真实命令。覆盖默认 User、普通 make:model、带字段生成、应用自定义 stub、已有生成模型默认拒绝覆盖；分别核对 Sanctum／Passport／Fortify 安装前后与重复安装，保留原 User 定制、自定义 stub、其他模型和 Action 定制。
- 生成模型手动接入验收应用模块后实际编译。临时应用的共享服务使用真实 User 创建、邮箱验证状态委托、回读及删除；原生 Artisan 命令与 TCP HTTP 路由分别消费同一服务。消费者位于测试应用 services，未把测试数据库流程塞回模型文件。安装后的能力 trait、生成字段及自定义方法均有真实 Rust 消费者。
- 这组断言约束当前交付模板和安装器的变更边界，不宣称能自动判断任意用户自定义 stub 的业务语义，也不代替全部认证协议与包功能验收。

## 实际结果

使用基线 `c14b562f8` 加本批测试的隔离源码，路径为项目 `target/tmp/model-layering-optional-20261124/source`；生成应用位于同一工作流 `generated`。Cargo 通过 `tests/build_guard.py` 使用 Rust 1.94.0、工作流独占缓存和项目内 TMPDIR，保持正常增量与并发；其他协作者的并行测试／请求输入等在途修改未纳入。

```sh
python3 -B tests/generated_model_layering_contract_test.py
python3 -B tests/build_guard.py --workflow model-layering-optional-20261124 -- python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/model-layering-optional-20261124/source/tests/generated_model_layering.py
```

- 分层契约单元测试7项通过：默认 User／内置模型模板／Fortify 发布路径，以及正反输出、必需能力与隐藏字段、旧存储定制保留、普通／raw 字符串保留、重复与越界声明。
- 独占 MySQL 8.4.13 生成应用全流程通过。初次按默认 Cargo feature 构建，未启用可选包；实际 `/up`、`/` HTTP 成功。随后分别执行 Sanctum、Passport、Fortify 的真实安装命令，核对安装前后白名单差异、必需能力、重复安装模型完全不变，以及既有 User、自定义 stub、其他模型和 Action 定制保留。
- `make:model` 的普通、带字段／具名连接、自定义 stub 产物均通过模型断言并接入实际应用编译；再次生成已有定制模型按预期失败且原文件不变。结构读取仍在生成命令中，生成模型只包含映射与自身行为。
- 完成安装后真实 Artisan 执行迁移、状态查询、路由查询和 `layering:probe`。该命令与 HTTP `/layering-probe` 均通过共享测试服务，在数据库实际创建 User、调用公开验证状态委托、回读确认并删除，返回生成表名、字段常量和定制方法的预期值；匿名访问 `/api/user` 返回401。测试消费者保留在生成应用 services／命令／路由中，未移入模型。
- 生成应用的 `cargo fmt --all`、`cargo fmt --all --check` 和 `cargo clippy --locked --features sanctum,passport,fortify --bin main -- -D warnings` 通过，随后包迁移回滚通过。没有提高递归上限或放宽 lint。
- 本轮是7项 Python 契约单测加一套真实生成应用验收，不把22次顶层编排命令、安装器子进程或旧测试计数累加成 Rust 测试数。运行期实现与消费者均为 Rust，Python 仅沿用项目验收编排方式。本轮没有新增 SQLite／PostgreSQL 的生成应用结论，也没有重新执行完整认证／Passkeys 协议验收。

### 夹具修正与收尾

最初夹具未完成 Fortify 必需的无界面／邮箱验证配置，后按现有 `tests/fortify_install.py` 配置方式修正；自选长表前缀产生75字符索引名，在独占 MySQL 中复现1059后改为短前缀 `ml_`。另修正了将 `Output::info` 样式输出直接当 JSON 读取的问题，测试命令改用原生 `write_raw`。严格 Clippy 曾发现测试命令嵌套 ORM Future 的布局深度过大，改用既有 `HttpFuture` 装箱测试服务边界后复验通过。这些修改均限测试夹具，没有调整生产模板、安装器、数据库规则或用户模型。

日志位于同工作流的 `generated-layering-*.log`，失败尝试单独保留，不计入最终通过结果。独占 MySQL 容器已回收，开发数据库和数据卷保留；收尾删除生成应用、隔离源码及其运行产物，保留日志和可复用编译缓存。只本地提交本批测试及验收记录，不推送；未修改三语开发者正文或业务代码。

后续复核发现的词法边界与隐藏字段位置问题已进入[语法解析与声明定位回归](syntax-regression-2026-11-24.md)；本页保留原实现和当时的执行结果，当前检查器实现与新验收以该记录为准。
