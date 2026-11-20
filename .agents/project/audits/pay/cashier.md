# Stripe 与 Paddle 订阅计费

2026-11-21，用户要求补齐完整订阅计费扩展，替代国内支付首期中 Stripe／Paddle 后置的停点。独立 `openrust-cashier` 与既有 `openrust-pay` 可同时启用；后者继续负责微信／支付宝一次性支付，不以两家国际供应商的计费模型代替国内支付协议。

## 固定调研与复用

官方网页搜索工具返回空结果，实际联网通过 GitHub API、官方仓库原始源码和官方文档核对；固定参考保留在 `/private/tmp/openrust-cashier-20261121/upstream`。不以 Star 数判断最优，不复制整个 PHP SDK，也不引入 PHP 生产依赖。

- [Laravel Cashier Stripe](https://github.com/laravel/cashier-stripe/tree/d5c015490ff77f2c138fe79b86d2a982787fe3ac)，MIT，16.11.0：借鉴 Billable、SubscriptionBuilder、Subscription 的客户／价格／试用／按比例结算／取消与恢复／支付方式及账单职责。
- [Laravel Cashier Paddle](https://github.com/laravel/cashier-paddle/tree/82bfc68eb72c38e666a4900baeecca3621840259)，MIT：借鉴 Paddle Billing 的托管交易、门户、订阅暂停／恢复、调整单退款；不混入 Paddle Classic。
- [Stripe 官方 PHP SDK](https://github.com/stripe/stripe-php/tree/23bedef75852e0adefc3f64713c7886cfb9f5e05)，MIT：固定 API `2026-11-18.endive`，核对 form 编码、订阅 items 与新版 invoice parent、Webhook 原始字节签名。
- [Paddle 官方 PHP SDK](https://github.com/PaddleHQ/paddle-php-sdk/tree/10359e49f4582bbfbcbe1bd3308a39593202d394)，Apache-2.0：核对 Billing v1、JSON、分页及原始字节 Webhook HMAC。
- [Laravel 文档固定源码](https://github.com/laravel/docs/tree/960eedf0911155ad16326ade49c0ea5d18eda0b8)：对照 billing 与 cashier-paddle 章节。另核对 Stripe API versioning、Paddle update-subscription 与 signature-verification 官方页面。猜测的 Paddle request-id 文档地址返回404，未将其作为幂等保证。
- `git ls-tree -r v0.x` 未找到 Cashier／Stripe／Paddle 旧实现；复用现有 HTTP Factory、包发现、Facade 作用域、发布注册和 SQL 事务，不新增 HTTP／缓存框架。

Rust 不能使用 PHP Eloquent trait／动态调用／Blade，采用显式绑定可信应用身份的 `Billable`、类型化订阅／价格与异步 `Result`。使用供应商托管页面、客户端 SDK 和原生账单 PDF，不在服务器收集卡号。公开名字跟随 Laravel；支持范围按三语正文说明，不声称每个 PHP 方法或供应商所有端点都已逐项封装。

## 设计与公开范围

根 `cashier` feature 经已有包发现注册 CashierServiceProvider。包内 `config/cashier.rs` 为唯一默认配置及发布模板，使用公共 `configurationIsCached()` 跳过缓存命中后的环境解析；`cashier-config` 标签复用现有发布机制。默认根配置不预装本扩展。应用显式发布后登记配置模块并重建，重复发布保留定制。Provider 启动仅绑定服务和迁移，不访问计费表或商户网络、懒检查 API 密钥。

CashierManager／Facade 按具名账户复用、forgetDrivers 显式刷新，已有 Client 保留密钥快照。账户名稳定对应同一商户及测试／生产模式；变更账户必须使用独立名字或数据库。API 固定版本、15秒超时、1MiB响应、禁止重定向、不自动重试；错误不含供应商正文。Stripe 使用稳定操作键的账户级 SHA256 作为 Idempotency-Key，避免原始业务键泄漏；HTTP 观察中请求／响应正文和 Authorization 脱敏。核心现有观察机制未保证删除所有标记 sensitive 的头，不能声称哈希头完全不可见。

客户创建／显式关联、一次性及订阅 Checkout、客户门户；Stripe PaymentIntent／SetupIntent／离线付款与 SCA 原生状态、支付方式、税号、余额及流水、Meter Events；两家订阅多价格／数量／折扣／预览／按比例结算／取消恢复，Stripe 试用及收款暂停、Paddle 暂停／激活／订阅一次性收费／更换付款方式交易；账单／分页／PDF／退款与部分退款。Paddle 试用来自价格目录；Stripe 专属 API 用于 Paddle 时明确 Unsupported。

五张包迁移表保存客户唯一映射、最小订阅与账单投影、事件身份／指纹／状态、Paddle 操作认领，不存卡号、地址、client_secret、API密钥及原始操作参数。高层对象操作校验远端 customer 归属，应用身份不从邮箱或前端参数猜测；高级 api 入口和 options 只供可信服务端组装。

Paddle 没有 Stripe 式幂等承诺：高层变更在持久认领后只发送一次；同键包括已完成、取消或崩溃后重试均拒绝再次发送，结果未知先查远端，核实后才生成新业务操作键。operationStatus 暴露 pending／done／rejected／unknown，不保存敏感响应以伪装安全重试。

Webhook 使用原始字节 HMAC，支持多签名与可信密钥轮换，拒绝重复签名头、多时间戳、超时、未来超窗、模式不符与 Stripe Connect。Stripe 300秒、Paddle5秒。事件行写锁与客户锁内读取供应商当前资源、校验归属、更新投影、执行宿主SQL、标记done处于同一事务；失败或取消回滚，并发与旧通知不会退回新状态。处理器提供 currentSubscription／currentInvoice，应据此授权；data() 保留可能迟到的已验签原事件。外部履约使用宿主事务发件箱。

Stripe customer.deleted 在锁下重新核对映射与远端删除状态，原子停用客户和取消本地订阅，customerDeleted 通知宿主撤销权限；旧句柄预览亦重新检查归属。重新关联必须显式核验新客户，unique冲突不允许 MySQL upsert 抢占其他用户映射。

## 验收环境与边界

隔离目录 `/private/tmp/openrust-cashier-20261121`，初始归档 `81dd7a14883775d4e12cd6e4a450d99ddac81959`；最终组合刷新到已提交的核心配置修正 `285fcacd041492c1d31624cc8f68adfb0c171c1e` 并叠加本扩展，使用独占 target、Rust1.94、增量关闭、jobs=3、dev/test debug=0。仅测试本批和受影响入口，不将全workspace Clippy计为所有模块行为测试。

初次曾误从活跃树启动 Cargo，产物仍在独占target，发现后立即终止，后续转到隔离源码。早期迭代错误及严格Clippy问题均修复并复验；最终组合首次刷新遇到Git重命名列表解析与mtime导致旧feature产物问题，修正为禁用rename的提交文件刷新及新mtime，重新验证。失败日志保留，不将失败或跳过计为通过。

真实商户凭据、产品权限、真实 Checkout／门户／Stripe.js或Paddle.js／SCA、公开回调网络及资金交易未验；本地协议模拟和官方签名对照不能代替厂商生产验收。未改开发数据库、缓存或数据卷；不把 SQL 事务测试计为缓存三驱动专项。本批API指南已更新，完整Rustdoc站点导出由核心统一核对源码指纹，本批不复用历史快照为新包已导出。

## 最终验收结果

最终实现、包配置发布与客户删除修正按以下实际结果验收；日志保留在上述隔离目录。

- `cargo +1.94.0 test --locked --offline -p openrust-cashier --all-targets`：12个常规测试函数通过（persistence1、protocol7、provider2、webhooks2），示例编译。协议覆盖两家主要流程、SCA原生非成功状态、部分退款、分页、归属拒绝、15秒真实超时只一次请求、错误及HTTP观察脱敏。日志 `tests-final-combination.log`。官方样本函数在常规运行中为ignored，以下单独执行通过，不计为常规已执行。
- 固定官方PHP源码真正调用 Stripe WebhookSignature::computeSignature／verifyHeader、Paddle PaddleSignature::calculateHMAC／parse／verify，禁网、只读既有PHP8.5镜像 `50956f094e90` 生成当前时间签名后立即运行已编译的Rust独立函数，通过（`official-reference-combined.log`）。篡改Util.php被源SHA守卫拒绝（`reference-negative.log`）；不是用本包重写算法自行充当独立对照。
- 同一个persistence函数分别在 SQLite3.54.1、PostgreSQL180006、MySQL8.4.13通过：原生五表迁移与回滚、处理器错误回滚、future取消后恢复、八个并发事件仅一次处理、乱序通知读取当前状态、同事件异内容冲突、远端归属、最小账单投影、Paddle八并发仅一发送／同键变参拒绝、客户删除后的授权撤销与旧句柄预览拒绝。日志 `tests-final-combination.log`／`persistence-{pgsql,mysql}-combined.log`。三库使用隔离文件或正式临时容器入口及必需连接标志。
- 根 `--features cashier --test cashier_configuration --test application_skeleton --test configuration` 共4函数通过，包含真实独立main进程执行 `vendor:publish --tag=cashier-config`／重复发布保留修改／force恢复原模板，缺API凭据下 migrate、重复迁移保留events数据、status、config:cache后环境故意设为无效仍能启动、clear、rollback。该原生流程在上述PG18／MySQL8再次通过（`runtime-{pgsql,mysql}-combined.log`）。无生产密钥、不接开发数据库。
- `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` 与 `cargo +1.94.0 fmt --all --check` 最终退出0（`clippy-final-combination.log`／`fmt-final-combination.log`）。既有第三方vendor依赖仍有其自身告警，不声称整份构建日志零warning。
- 三语静态正文／链接检查、VitePress构建通过；两页×三语×两挂载×桌面移动共24组Cashier页面、通用12组页面与全部导航6组通过，支持说明页计数由26降至24。日志 `docs-content-combined.log`／`docs-build-combined.log`／`docs-cashier-combined.log`／`docs-browser-combined.log`／`docs-scope-combined.log`。示例作为包examples实际编译，API全站导出不在此项。

本批收尾未修改框架公共API、安装器、根config目录或其他扩展源码；核心配置修复来自上述独立提交。暂存仅本包、Cargo接线与锁、根定向测试、三语计费正文／国内支付交叉引用、文档测试和本主题记录，保留其他未跟踪记忆文件。仅本地提交，不推送。

关闭 Cashier 的默认feature骨架／根配置3函数另行通过（`feature-off-combined.log`）。最终34份本批编译／文档输入逐字节与活跃工作区一致。组合初轮隔离目录残留早期未跟踪根config/cashier.rs被默认骨架回归拒绝，已删除本批隔离残留后重跑通过；活跃树不存在该文件。

## 产物清理

所有本批构建、浏览器和临时数据库入口退出，target无打开文件后，删除独占target与隔离站点dist／cache，仅unlink共享node_modules符号链接；源码、固定上游、日志、共享依赖、开发容器和全部数据卷保留。`df -kP .` 可用从44,074,988 KiB增至48,728,212 KiB，净增加4,653,224 KiB；并行磁盘活动可能影响净值。Docker卷清单前后相同（为空），不删除开发数据库或卷。清理明细留在临时cleanup-cashier.json。
