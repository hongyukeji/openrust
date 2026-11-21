# 扩展复用取舍（2026-11-21）

适用范围：本次全包审计 R6；不升级固定上游，不以仓库热度代替契约核对。联网读取 GitHub 固定源码及候选 manifest，失败路径不作为依据。原始响应在 `tmp/extension-design-audit-20261121/` 与 `tmp/extension-repairs-20261121/upstream/`。临时下载不是交付依赖，版本与可复核源码路径保留如下。

## Horizon、Telescope、Pulse 面板

| 方案 | 可直接复用部分 | 接入成本与具体差异 | 本次决定 |
|---|---|---|---|
| [Horizon b86d5aa](https://github.com/laravel/horizon/tree/b86d5aa87ca474db352ea2668bdd1df3a90bf8aa) 的 Vue 客户端 | 浏览器资源可在 Rust 服务端使用；MIT。`resources/js/app.js`、`routes.js`、`screens/` 已检查 | 初始化依赖 `window.Horizon.proxy_path/path`、Vue history 叶路由、JSON 详情结构及分页。上游从 meta 取 X-CSRF-TOKEN；当前原生面板还要求 X-Horizon-Request。当前 API 很多同路径，但 `screens/recentJobs/job.vue` 读取 payload.tags/payload.pushedAt 并解析 PHP 序列化 data.command；Rust JobDetails 将 tags 单独存放，Payload 为原生 handler/data。因此资源可以复用，但详情适配必须明确保留 Rust 载荷机制。需要补叶页面回退、初始化视图、资源发布及响应适配，并按所有屏幕重新验收 | 保留现有原生面板；这保留授权、nonce CSP 和已交付运维能力，避免同时维护 Vue 分支与原生分支。没有声称 Rust 无法运行 Vue，也没有声称原面板完整兼容。若交付完整上游 UI，优先接入固定官方资源并替换现有页面，不开发第二套组件系统 |
| [Telescope 39a8ea8](https://github.com/laravel/telescope/tree/39a8ea8ad79915df6c3ac9b4e72659ade808eba3) 的 Vue 客户端 | MIT；`resources/js/app.js`、`routes.js`、IndexScreen/PreviewScreen 可复用 | 上游请求 `telescope-api/toggle-recording`、`telescope-api/entries`，依赖 `window.Telescope.path/timezone/recording`、entries/related 展示协议、Bootstrap/Popper/moment-timezone、history 路由。现有 Rust 已有同路径 telescope-api 入口、CSRF 处理和 history 页面回退，这些是复用的有利条件；无需另造这些后端入口。当前页面未注入 window.Telescope 初始化对象，只嵌入原生脚本，因此不能只换 JS 文件。仍须按 18 类详情逐项验证 Rust 数据、初始化视图、资源发布和 CSP，而非假设同名 Watcher 的任意字段可直接使用 | 保留现有单页安全页面，复用 Watcher/Repository/事件机制；完整官方 UI 若成为交付要求，先做官方资产与 API 适配，而非重写组件。此决定接受原生页面维护成本，不宣称其功能超过官方客户端 |
| [Pulse 33cc6fc](https://github.com/laravel/pulse/tree/33cc6fc68cd3052aa06227e88e8d933dd3d70a51) 的 Livewire | 卡片语义、聚合、Recorder 和存储设计可复用 | `src/Livewire/` 与 Blade/Livewire 请求依赖 PHP 组件运行时；纯资源复制不能运行这些生命周期。在 Rust 中模拟整套 Livewire 超过本扩展范围 | 保留原生页面与原生 Recorder/Storage，不新建 PHP 兼容运行时；公开说明不是完整 Livewire UI |

现有 Horizon 页面为 8,760 字节、Telescope 为 11,072 字节（以本次实际文件为准），均用 include_str! 单页嵌入而无独立包前端构建。上游页面可浏览器执行，也可采用发布资产避免源码构建；若引入，仍需维护资产版本、初始化及详情行为，不能把“不想维护 npm”写成技术不可能。

三者均保留既有源码许可证。原生面板只承担包内观测/控制，不演变为通用后台框架。选择依据为现有接口与生命周期差异，未作原生面板优于 Vue 的性能声明。当前 UI 已有浏览器验收，本次 Horizon 配置路由变更仍须重新验收授权、可变路径和写入行为。

## OIDC 客户端

比较 [openidconnect-rs e20ddbb](https://github.com/ramosbugs/openidconnect-rs/tree/e20ddbb01ca68356f2979bc125f5613ab45b4e7e)：manifest 4.1.0、Rust 1.88、MIT，满足当前 Rust 1.94。默认 reqwest/rustls 可关闭；`src/lib.rs` 明确支持自定义 HTTP，因此“强制另一 HTTP 栈”不是不采用理由。

其价值是成熟的 Discovery/类型化 Claims/ID Token 验证和 OAuth2 状态机；代价不是库不能做，而是当前 `oidc/src/provider.rs` 的原生 Session 一次性 attempt、缓存原子状态消费、配置指纹、账户绑定、注销撤销和本地登录仍须保留。HTTP 可适配现有 Factory；验签部分则要与当前 `claims.rs` 的 jsonwebtoken/AWS-LC 路径进行替换核对。候选不可选关闭的 crypto 依赖含 rsa/p256/p384/ed25519-dalek/hmac/sha2，时间依赖含 chrono；现有包复用已在工作区使用的 jsonwebtoken/AWS-LC/Jiff，不增加这些依赖。不能把这些差异写成库不安全或无法集成。

决定：本次不更换协议内核。已声明范围是 Authorization Code + PKCE 与既定 JWT 策略，新增依赖不能替代 Session/账户/注销的大部分集成，整合同时保留两套 verifier 会增加维护面；保留一条验签路径及明确算法/issuer/audience/nonce/at_hash 策略。自有 Discovery/Claims 验证有长期维护责任；若扩展新 grant、加密 ID Token、复杂 metadata/provider 兼容，先重新评估以该库替换协议层，不能继续无比较扩张。Jumbojett PHP OIDC 是协议设计参考，不能作为 Rust 依赖。

本次没有修改 OIDC 生产代码；既有 flow/sso 的本地协议证据仍是原有边界，库选型记录不冒充新增外部身份商验收。

## Cashier / Stripe

比较 [async-stripe 19d6990](https://github.com/arlyon/async-stripe/tree/19d6990265e23d1257602696ea065376c07d802b)：workspace 1.0.0-rc.10、Rust 1.89、MIT OR Apache-2.0；客户端、生成类型和 Webhook 分包，可只选子集。不能称其必须整体引入。

具体源码：`generated/async-stripe-shared/src/version.rs` 默认 `V2026_10_28_endive`，本项目官方 stripe-php 固定 API 为 `2026-11-18.endive`。即使自定义请求版本 Header，也不能据此证明旧版本生成类型覆盖固定新 schema。`async-stripe-webhook/Cargo.toml` 使用 hmac/sha2/chrono 及生成事件类型；它可以替代部分验签代码，但不会替代当前 Webhook 收件箱、幂等认领、客户归属、事务确认、事件乱序重拉及 Paddle 的未知结果认领。完整 client 路线还需将原生 HTTP Factory 的观察/脱敏/超时边界接入，不能静默绕过原生 HTTP 观察。

决定：保留现有原生 Client 与类型化本地记录；此次修正的是上游可表达原名、供应商边界、配置键及使用文档。保留厂商对象 JSON 作为明确适配，避免声称是官方 SDK 类型。若候选发布稳定且生成 schema 对齐本项目固定 API，先评估以类型或 Webhook 子包替换相应层，再测量是否减少总维护代码；不因候选存在就复制一套 HTTP/存储。未做 SDK 性能优劣声明。

## 旧版复用核对

只读检查同级 `openrust-v0.x`：未发现本次 Horizon/Cashier/Activitylog/Scout 同名包可直接提取；继续复用主线已有 Router/Provider/HTTP/Redis/Eloquent/事务，不新增管理器、业务表或生产依赖。旧版来源提交与已有 Pay/OIDC 复用仍见对应主题记录。
