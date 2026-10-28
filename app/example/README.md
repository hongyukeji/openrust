# 综合业务示例

> 迁移边界：本文代码与链接描述当前 `app/example/` 实现；最新目标为 `modules/example/`，按 [ADR-074](../../.agents/project/decisions/adr-074-laravel-modules-directory.md) 随统一模块契约迁移。当前 Provider 装配不表示 module 管理已完成。

`app/example` 是根应用挂载的普通业务模块，供开发者学习如何在 `app/<业务>/` 调用框架公开 API，以及通过公开 Provider/driver 契约扩展框架。框架和应用都在本仓 `main` 开发；这里没有独立 package、bin、启动模式或 example feature，也不复制完整应用骨架。

**当前正在补齐示例与根布局验收。** 目标是贯穿所有当前已实现的公开框架能力；基础路由、旧认证与任务代码迁移不代表该目标完成。完整业务 CRUD、其他能力的实际示例代码和新布局门禁仍有缺口，详见下表。

模块扩展目标参考用户指定的第三方 [nWidart/laravel-modules](https://github.com/nWidart/laravel-modules)；当前依据为 [v13 文档](https://laravelmodules.com/docs/13/getting-started/introduction)，读取日期 2026-10-28，未宣称源码 revision 已对齐。借鉴自有路由/API、controller/model/resource、迁移、测试与 provider 装配；它不是 Laravel 核心，不扩张 Laravel 主基线，也不表示本框架已全量实现该包。按本项目约定采用 app/<模块>、snake_case 和编译期静态注册。修改 app/mod.rs 的模块声明和应用注册清单是正常业务接入；为某个业务修改 src/ 核心或加入全局样例特判不属于接入方式。优先复用现有 ServiceProvider/Routes；Rust 模块编译期声明，无运行时源码扫描。

## 从真实调用链开始

根 [`src/main.rs`](../../src/main.rs) 静态声明 app、bootstrap 和通用 routes，然后运行 `bootstrap::Server`。[`bootstrap/app.rs`](../../bootstrap/app.rs) 在 Provider 清单中注册 [`ExampleServiceProvider`](providers/example.rs)。模块在 `boot` 内通过 `BootContext::routes` 贡献 [`routes::register`](routes/mod.rs)；应用清单按已启用服务注册迁移、种子和任务。根 [`routes/web.rs`](../../routes/web.rs) 只保留框架通用路由，不硬编码业务 handler。

模块 `GET /api/example` 由 `show` 返回 `Json<ExampleInfo>`，其中 `message` 是示例文字，`served_at` 直接调用第三方 `chrono::Utc::now()`；类型通过 serde 序列化，经过框架 HTTP 处理链。
从仓库根启动，在另一终端请求默认端口：

```bash
cargo run
```

```bash
curl --noproxy '*' http://127.0.0.1:3000/api/example
curl --noproxy '*' http://127.0.0.1:3000/health
```

默认基础示例不要求数据库或 Redis；端口与公开目录见根 [`config/server.toml`](../../config/server.toml)。这条路径展示挂载与响应，尚不是完整 CRUD。请求结果只在实际执行门禁后记为通过。

## 新建自己的业务模块

以 `app/orders/mod.rs` 为例，在 Provider boot 阶段贡献路由：

```rust
use openrust::prelude::*;
use openrust::core::{BootContext, BoxFuture, ServiceProvider};

pub(crate) struct OrdersServiceProvider;
impl ServiceProvider for OrdersServiceProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.routes(register)?; Ok(()) })
    }
}
fn register(routes: Routes) -> Routes { routes.get("/api/orders", index) }
async fn index() -> Json<Vec<&'static str>> { Json(vec!["first-order"]) }
```

在根 `app/mod.rs` 声明模块，并将 `OrdersServiceProvider` 加入应用的 Provider 清单。业务路由留在业务模块，示例静态列表只说明注册，不代替完整 CRUD。register 阶段绑定服务，所有 Provider 注册后依序 boot；第三方依赖由 Cargo 解析。模块启停与管理仍待后续实现。

## 模块里各类代码写在哪里

以下位置已存在，按实际职责拆分；没有业务实现的 models/services 等目录不预建：

| 实际位置 | 写什么 |
| --- | --- |
| mod.rs | 模块声明，保持薄入口 |
| providers/example.rs | 通过 ServiceProvider 贡献业务路由 |
| routes/api.rs、routes/web.rs | API/页面路由组合，调用 controller |
| http/controllers/example.rs、auth.rs、jobs.rs、pages.rs | 实际请求处理 |
| http/requests/auth.rs | 当前认证输入 DTO；完整业务校验仍待 CRUD 补齐 |
| http/resources/example.rs、credentials.rs、about.rs | 示例信息、认证输出、页面 context |
| database/migrations/mod.rs、database/seeders/mod.rs | 模块业务迁移与种子 |
| resources/views/ | 业务模板 |
| jobs/ | 当前心跳任务和调度插件 |
| extensions.rs | 缓存/通信公开 Provider 扩展参考；尚未接入生产清单 |
| tests/auth_flows.rs、tests/registration.rs | 同一根 Server 认证流程、模块注册与路由边界测试 |
| proto/、generated/、grpc.rs | 实际协议输入、显式生成脚本与已生成类型；完整 RPC 服务示例仍待补 |

新增完整 CRUD 的实体放 models，校验/资源分别放 http/requests 与 http/resources；只有确需业务编排时再加 services。events/listeners/notifications 按实际能力落地，不提前堆空层；它们是后续约定，不代表当前代码已覆盖。

## 使用业务自己的第三方依赖

`app/example` 是普通 Rust 模块，不是 Cargo package；在其子目录放 Cargo.toml 不会自动产生独立依赖边界。当前根 openrust package 同时构建框架 lib 和 server，业务依赖声明写在根应用 Cargo.toml，lib/server 的依赖解析与 feature 选择存在共享影响。“无需改框架源码”允许修改应用依赖与模块注册清单，不能解释成依赖图完全不变。

例如仓库已经登记并直接继承 serde、serde_json 和 chrono：

```toml
# 根 Cargo.toml 中的既有条目，不要重复追加
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }

[dependencies]
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
```

`http/controllers/example.rs::show` 已实际调用 `chrono::Utc::now()`，`http/resources/example.rs::ExampleInfo` 保存 `chrono::DateTime<chrono::Utc>` 并 derive `serde::Serialize`；认证输入也 derive `serde::Deserialize`。业务可直接导入这些已声明的包；下面是可用于新业务模块的序列化用法，不需要再添加依赖：

```rust
use serde::Serialize;

#[derive(Serialize)]
struct OrderLabel {
    name: String,
}

fn encode_label(label: &OrderLabel) -> Result<String, serde_json::Error> {
    serde_json::to_string(label)
}
```

真正新增业务包时，先确认包名、版本要求、许可证与所需 feature。若根 `[workspace.dependencies]` 已有同包，优先在根 `[dependencies]` 通过 `workspace = true` 继承；否则在前者登记核实后的明确版本约束，再在后者继承，不为教学随意引入生产依赖或编造版本。只用于测试的包写根 `[dev-dependencies]`；公共复用的版本政策同步 stack.md。声明位置和版本规则见 [Cargo 依赖说明](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)。

只启用业务需要的 feature；`default-features` 按实际需要设置，继承项的默认 feature 策略应从 workspace 声明协调。不要随意改框架依赖版本或默认 feature，不用无差别 `cargo update`，也不以 `[patch]` 强行覆盖核心依赖。package alias 解决导入名称或同时引用多版本的命名问题，不隔离同一依赖的 feature，也不会消除类型/解析冲突；Cargo 的 feature 合并规则仍生效。见 [Cargo features](https://doc.rust-lang.org/cargo/reference/features.html)。

审查根 Cargo.toml/Cargo.lock 的实际差异，先使锁文件精确反映获准的清单变动，再验证锁定结果：

```bash
git diff -- Cargo.toml Cargo.lock
cargo tree --locked -d
cargo tree --locked -e features
cargo check --locked -p openrust --all-targets
cargo test --locked -p openrust
cargo deny check
```

重复版本和 feature 来源可按 [cargo tree](https://doc.rust-lang.org/cargo/commands/cargo-tree.html) 继续定位。以上是验证步骤，不表示本轮已替开发者添加新包或执行过依赖演练。示例协议/生成脚本仍留模块内，不能因为第三方包需要代码生成就新增示例专用根 build.rs/build 依赖。

若将来确需更强的业务依赖隔离，同仓 workspace 的独立 package 可以另行设计；当前 root lib 与示例若相互依赖会产生环，普通目录也不是 feature 隔离。本轮不实施第二套 package 架构，不承诺完全独立的业务依赖图；任何此类调整须先收敛依赖方向及验收。

## 通用挂载与模块资源边界

除 app/example 外都是框架结构与代码。模块专属代码、迁移、种子、模板、协议、生成脚本及测试 fixture 全放模块内；根职责目录只提供通用框架结构与公开加载/挂载，不存样例业务或示例专属分支。模块可按实际需要有资源目录，但不复制一套完整框架骨架。

| 职责 | 边界 |
| --- | --- |
| 通用启动、模块声明与公开挂载 | 根 app/mod.rs、bootstrap 和 routes；核心库不依赖业务 |
| 框架通用配置与加载 | 根 config；示例专属配置留模块并显式通过公开 API 接入 |
| 示例业务迁移、种子与数据 | app/example 内实际业务文件；根 database 仅通用结构/挂载 |
| 示例模板、语言和其他资源 | app/example 内实际需要的资源；根 resources/public 仅通用结构/加载 |
| 示例协议与生成脚本 | app/example 内实际需要的协议/脚本；根不因示例增加 build.rs 或 build 依赖 |
| 示例专属测试与 fixture | 模块内；根 tests 可保留通用验收并通过公开 API 挂载模块 |

示例实际迁移/种子在 database/migrations/mod.rs 与 database/seeders/mod.rs，模板在 resources/views，测试在 tests/auth_flows.rs 与 tests/registration.rs；这些均位于本模块内。协议输入为 proto/echo.proto，显式生成脚本为 proto/generate.py，工具清单与锁固定在 proto/generator.toml 和 proto/generator.lock，已生成类型为 generated/echo.rs；脚本在 /tmp 临时目录执行 cargo run --offline --locked，根没有为示例保留 build.rs/build 依赖。模块整理后的 all-features/all-target Clippy 已由实现者实际执行通过，协议生成脚本也已实际运行；完整新布局测试/文档/双库/HTTP 门禁仍在执行，不能将局部门禁或路径归位当作完整行为验收。
数据库、认证、队列等按现有 Cargo feature 和根配置启用。根应用检查显式数据库声明后装配数据库基础设施；没有声明表示未启用数据库业务，配置已声明但依赖不可用时必须报错，不回退到内存或空实现。模板通过 `view` 启用，并显式设置 `APP_VIEW_DIRECTORY=app/example/resources/views`；Protobuf 已生成类型按 `grpc` 编译，修改 proto 后显式运行 `python3 app/example/proto/generate.py`，通过 `python3 app/example/proto/generate.py --check` 比较重生字节；启用 feature 本身不等于已注册服务或实际监听端口。

## 当前能力覆盖与缺口

“代码存在”仅说明当前可见实现，不能替代本布局 check/test、真实请求与外部服务验收。根基础设施装配、文档链接或 consumer 测试也不自动算作 `app/example` 的业务代码覆盖。

| 能力 | 本模块实际代码/公开扩展 | 根接入与条件 | 测试/当前缺口 |
| --- | --- | --- | --- |
| 完整业务实体 CRUD | 待补列表、详情、新增、更新、删除及业务模型 | 模块内业务迁移经公开入口挂载；PG18/MySQL8.4；真实 HTTP | 校验、分页、未找到、授权、事务与两库完整闭环均待补，基础常量路由不算完成 |
| HTTP 路由与 JSON 响应 | `routes/api.rs` + `http/controllers/example.rs::show`，公开 `Routes/Json` | ExampleServiceProvider 公开贡献模块 routes，包含 `/api/example` | 根 server 路由回归；本布局结果待记录，复杂响应/资源/限流示例仍待补 |
| 配置、应用生命周期、命令 | 本模块尚无完整业务与扩展演示 | 根 bootstrap、config 与唯一 server | 仅根装配不算完整示例覆盖；公开自定义命令、配置错误及退出行为参考待补 |
| 数据库、迁移、种子、事务、查询 | 认证流程间接使用数据库；独立业务查询与事务待补 | 模块 database/migrations 与 database/seeders，ExampleServiceProvider 通过公开插件挂载；对应 db feature 与真实服务 | CRUD/查询/事务业务示例待补；旧框架测试不代替本模块覆盖 |
| JWT/PAT 与认证流程 | `routes/api.rs` + `http/controllers/auth.rs` 调用 `AuthFlows/AuthLayer/CurrentUser` | `auth`、对应 db feature；通用 auth 配置与模块业务迁移、授权注入密钥 | `app/example/tests/auth_flows.rs` 复用同 Server；当前布局双库结果待记录；Policy/MFA/OIDC/Session 业务参考仍待补 |
| 邮件及公开邮件实现 | `http/controllers/auth.rs::AuthCodeEmail` 实现 `Email`，`send_auth_code` 使用 `Mailer` | 根 mail 配置与 `MailerServiceProvider` | 认证测试有捕获 MailDriver；SMTP/真实发送须独立授权和条件，不能从 fake 推断 |
| 自定义缓存/邮件/SMS driver | 待补通过统一 Provider 与公开 driver 契约接入的业务示例 | 示例配置与实现留模块，通过通用入口注册 | 注册、TTL、配置拒绝、捕获通信与生产环境保护等行为待实现并实测；不以其他 consumer 替代 |
| 队列与调度 | `jobs::JobsServiceProvider`、`ExampleHeartbeatJob`；路由派发任务 | `queue`、数据库与模块迁移的公开挂载；API/worker/scheduler 为同一应用的进程角色 | heartbeat 通过公开 Provider 注册 scoped `HeartbeatRun`，每个 attempt 解析独立关联号，关闭时记录完成；内嵌测试以公开 `JobRegistry::execute` 验证两次 attempt 不复用。真实持久化派发/消费、失败/重试/事务业务示例仍待补 |
| 事件与通知 | 待补公开事件/通知业务调用 | 按既有 events/notifications 契约与 feature | 本模块无完整业务链，不以框架测试代替 |
| 缓存、tag/SWR、并发加载 | 待补真实业务缓存及驱动扩展 | 现有缓存配置与选定 store；Redis 条件明确 | 根安装 CacheServiceProvider 不等于示例已调用，业务与扩展测试待补 |
| 本地/S3 存储与流式上传下载 | 待补业务路由、授权、公开 Storage 调用 | storage feature、根配置及适用真实后端 | 既有 storage_http/S3 测试是框架证据，尚不算本模块示例 |
| 服务端视图 | `http/controllers/pages.rs::about_page` 注入 `Views` | `view`；模块 resources/views，显式 APP_VIEW_DIRECTORY 配置与公开视图挂载 | `/pages/about` 代码存在，新布局真实页面与错误场景待记录 |
| i18n 与地区格式化 | 待补业务 locale/消息/格式化调用 | 根 i18n 插件与资源 | 根安装插件不等于模块覆盖 |
| 加密与签名链接 | 待补公开 API 的业务加解密/签名验证 | 对应 feature、根密钥与配置，敏感值不进入文档 | 根插件和认证测试密钥不是完整业务示例，仍待补 |
| 审计、租户、权限边界 | 待补业务审计/租户传播/资源授权代码 | 通用数据库配置、模块业务迁移，公开 API | 仅根 AuditServiceProvider 装配不算覆盖，事务/隔离测试待补 |
| HTTP 出站与上下文 | 待补公开客户端、预算/取消与失败语义 | http-client 等已有 feature、受控本地服务 | 真实请求、超时/取消与传播演示待补 |
| WebSocket/SSE 与跨节点 | 待补业务路由与公开 transport 使用 | realtime feature；Redis 跨节点条件 | 其他 consumer 的真实拓扑不代替本模块代码 |
| gRPC 与 HTTP/gRPC 组合 | `grpc.rs`、`proto/echo.proto` 仅保留类型构建 | 模块 proto/generate.py 显式生成 generated/echo.rs；根无示例构建脚本/依赖 | 目前没有本模块完整服务实现/注册与请求闭环，不能称 gRPC 示例已完成 |
| CSV/XLSX 与任务式数据交换 | 待补业务映射、授权、导入/导出/任务引用 | data-exchange 等现有 feature、根配置与存储 | 本模块业务和失败/边界测试待补 |
| 校验、分页与公开资源宏 | 待补完整实体的校验与分页/资源代码 | 公开 Validate/资源接口，复用业务 CRUD | 默认响应路由不足以证明，待 CRUD 同步覆盖 |
| 测试工具、替换与公开扩展 | 模块 tests/auth_flows.rs 使用 TestDatabase、捕获邮件 driver | 模块测试与通用根验收挂载、隔离数据库 | 不代表未来 TestApp/Factory 已实现；其余模块演示/断言待补 |
| 观测、就绪与故障行为 | 本模块业务观测/过载链待补 | 根 probes/metrics/otel 按现有配置启用 | 框架观测和服务测试独立保留，综合示例实际请求证据待补 |

此表随当前源码更新，未列入的公开能力也须对照根 `src/lib.rs`、Cargo features 与项目 core 事实继续盘点。尚未实现的 Laravel 能力留在框架计划；每个后继功能簇同步实际框架 API、综合业务代码、通用根挂载、模块专属资源与测试，不能把目录迁移当作全框架等价。

## 验证与扩展

根框架测试使用 `cargo test -p openrust`，完整工具门禁使用 `cargo test --workspace`。认证 target 为 example_auth_flows，路径为模块 tests/auth_flows.rs；需要对应 feature 和有建库权限的隔离测试 URL，没有 URL 时跳过不算真实数据库通过。实际命令与结果统一记录于 [测试基线](../../.agents/project/memory/testing.md)。

编写业务时使用公开 `Routes`、`Inject`、`AuthFlows`、`Queue` 等入口；当前可参考 `JobsServiceProvider` 的 Provider 注册 和 `AuthCodeEmail` 的公开 trait 实现。缓存与通信扩展参考源码尚未接入生产清单，实际挂载与验证仍待完成；进程内演示缓存不作为共享缓存，捕获通信仅允许显式本地/测试配置并拒绝 production。

自定义 driver 应实现框架已公开的 trait，通过应用 ServiceProvider 装配，不能修改框架核心来识别某个业务模块。其他 [公开 consumer](../../tests/consumers/README.md) 可辅助理解边界，但不能替代本模块的覆盖交付。
