# Cashier（Stripe）

## 安装与配置 {#configuration}

`openrust-cashier` 提供 Stripe 与 [Paddle Billing](cashier-paddle.md) 的客户、Checkout、订阅、账单与 Webhook。微信支付和支付宝使用[支付扩展](pay.md)。两个包可以同时启用；国内人民币金额类型不用于 Stripe 多币种计费。

根应用启用 `cashier` Cargo feature。独立应用添加 `openrust-cashier` 依赖并启用 OpenRust 包发现。`CashierServiceProvider` 自动绑定 `CashierManager` 并登记迁移，启动时不连接计费服务、不读取账单表。执行原生 `artisan migrate` 创建 `cashier_customers`、`cashier_subscriptions`、`cashier_invoices`、`cashier_events`、`cashier_operations`。

需要定制编译配置时，运行 `artisan vendor:publish --tag=cashier-config` 发布 `config/cashier.rs`。重复发布保留应用修改，只有 `--force` 才覆盖。在应用 `config/mod.rs` 声明 `mod cashier;`，并在配置组中加入 `("cashier".into(), cashier::load(env)?)`，然后重新编译应用。不发布配置也可使用包内环境默认值。

```dotenv
CASHIER_DRIVER=stripe
CASHIER_LIVE=false
STRIPE_SECRET=sk_test_...
STRIPE_WEBHOOK_SECRET=whsec_...
```

Stripe API 固定为 `2026-11-18.endive`；Webhook 使用相同版本。`CASHIER_LIVE=true` 用于生产，需同时替换 API 和 Webhook 密钥。密钥只在服务端保存。前端 Stripe.js 使用可公开的 publishable key，不能使用 `STRIPE_SECRET`。

包加载 `cashier.default`、Stripe 的 `cashier.key`／`cashier.secret`／`cashier.webhook.secret`，以及 Paddle 的 `cashier.paddle.api_key`／`cashier.paddle.webhook_secret`／`cashier.paddle.sandbox`。同一 Rust crate 使用 Paddle 配置命名空间区分两个上游包。`cashier.drivers` 保存显式具名账户，同名账户优先于供应商原生配置。在应用配置加载器返回 `cashier` 分组可覆盖默认值；应用分组优先，`drivers` 整组替换。增加具名账户时明确填写 `driver`、`secret`、`webhook_secret`、`live`。`webhook_secrets` 数组支持轮换。每个名字代表一个稳定的商户账户与模式；不要把已使用的名字重新分配给其他商户或从测试切换到生产，使用独立数据库或新账户名。

`Cashier::driver("stripe")`、`Cashier::default_driver()` 在应用作用域解析账户。`CashierManager::get_default_driver()` 返回配置名；`forget_drivers()` 后新解析实例加载新配置，已持有的实例保留原凭据快照。配置缓存启用时保留缓存快照，修改环境后需按[配置文档](../getting-started/configuration.md)重建缓存。

## 客户与 Checkout {#customers-and-checkout}

`Billable` 绑定经过应用认证的身份，不通过回调参数、邮箱或 Checkout 查询字符串推断用户。账户名与本地身份共同隔离客户映射，远端客户只能关联一次。

Stripe 客户接口保留 `stripe_id`、`has_stripe_id`、`create_as_stripe_customer`、`create_or_get_stripe_customer`、`update_stripe_customer`、`update_or_create_stripe_customer` 和 `as_stripe_customer(&["tax"])`。已存在客户时创建返回 `CustomerAlreadyCreated`；创建或获取会读取现有客户并传递 `expand`。`billing_portal_url(return_url, options, key)` 返回经过校验的 HTTPS 地址，`redirect_to_billing_portal` 返回 HTTP 302 响应。Stripe 专用接口在发送请求前拒绝 Paddle 账户。已有 `create_or_get_customer`、`update_customer` 和 `billing_portal` 保留为显式的跨供应商便利入口。

`Billable` 是绑定可信身份的适配器；客户姓名、邮箱、地址通过 `options` 提供，不反射应用模型字段。供应商对象返回 JSON，本地订阅和账单记录使用类型。`subscription()` 返回持有操作能力并解引用到 `Subscription` 数据的 `SubscriptionHandle`；`as_stripe_subscription` 与 `as_paddle_subscription` 分别校验供应商。`raw_balance()` 返回 Stripe 最小货币单位；现有整数 `balance()` 是便利别名，不提供 PHP 的本地化货币格式。修改请求显式传入业务幂等键以保证重试安全。

```rust
use openrust_cashier::{Billable, BillingStore, Price, facades::Cashier};
use serde_json::json;

let client = Cashier::driver("stripe")?;
let store = BillingStore::new(connection.clone());
let user = Billable::new(client.clone(), store.clone(), "users:42")?;
user.create_or_get_stripe_customer(json!({"email":"buyer@example.com"}), "customer:42").await?;
let session = user.new_subscription("default", vec![Price::new("price_monthly", 1)?])?
    .trial_days(14)?
    .with_coupon("coupon_welcome")?
    .checkout(json!({
        "success_url":"https://shop.example/success?session_id={CHECKOUT_SESSION_ID}",
        "cancel_url":"https://shop.example/account"
    }), "subscription:42:first").await?;
```

跳转至 Stripe 返回的 `url`。成功跳转只表示浏览器返回，应以经过签名并持久处理的 Webhook 同步付款和订阅状态。`checkout(prices, options, key)` 创建一次性 Checkout；订阅使用 `new_subscription(...).checkout(...)`。价格必须来自服务端认可的 Stripe 目录，禁止让客户端任意提交价格、金额或客户 ID。

`create_or_get_customer` 返回已关联客户或创建客户；并发调用必须复用同一稳定操作键。`attach_customer(id)` 只用于应用已经验证归属的既有客户或结果未知后的人工核对，不能把任意请求参数直接传入。`update_customer` 更新资料；`billing_portal(return_url, key)` 创建 Stripe 客户门户会话。

## 订阅生命周期 {#subscriptions}

`new_subscription(name, prices)` 支持多个价格、`Price::new(id, quantity)`、`Price::metered(id)`、`trial_days`、`trial_until`、`skip_trial`、`quantity`、`anchor_billing_cycle_on`、`with_metadata`、`with_coupon` 和 `with_options`。直接 `create(payment_method, key)` 使用 Stripe `default_incomplete`，传入支付方式时要求它已属于该客户。

```rust
use openrust_cashier::Proration;

let subscription = user.subscription("default").await?.ok_or("missing subscription")?;
let subscription = subscription.update_quantity(3, None, Proration::Invoice, "seats:42:v2").await?;
let subscription = subscription.cancel("cancel:42:v1").await?;
if subscription.on_grace_period() {
    subscription.resume("resume:42:v1").await?;
}
```

`swap(prices, proration, key)` 替换完整价格集合，移除旧项目并保留匹配项目 ID。`Proration::Prorate` 按比例计入下次账单，`Invoice` 立即结算，`None` 不按比例结算。多价格订阅调用 `update_quantity` 时明确指定价格。`cancel` 在计费周期结束时取消，`cancel_now` 立即取消；`resume` 恢复宽限期内或 Stripe 暂停的订阅。`extend_trial` 延长试用；`pause_collection`／`resume_collection` 控制 Stripe 收款暂停，收款暂停不等于订阅状态暂停。

`subscription(name)` 返回该名字当前订阅；若只有已终止订阅，返回最新记录。多个仍存在的同名订阅会明确失败，此时使用 `subscription_by_id(id)`。`sync_subscription(id)` 在核对客户归属后拉取远端状态；`refresh()` 刷新现有句柄。`subscribed`、`valid`、`active`、`on_trial`、`canceled`、`on_grace_period`、`paused`、`past_due`、`incomplete` 可用于应用权限判断；欠费或未完成付款默认不授予有效订阅权限。

`as_provider_subscription(parameters)` 获取核对归属后的原生对象。例如传入 `{"expand":["latest_invoice.confirmation_secret"]}` 获取直接创建订阅所需的付款确认信息；将适用的 client secret 交给该客户的 Stripe.js 完成认证。不要记录或跨用户返回 client secret。

## 付款、账单与税费 {#payments-and-invoices}

`create_setup_intent`、`payment_methods`、`add_payment_method`、`default_payment_method`、`update_default_payment_method`、`delete_payment_method` 管理 Stripe 支付方式。用 Stripe.js Payment Element 收集卡信息；服务端接口只接收 Stripe 标识符，不接收卡号或 CVC。

`pay(amount, currency, options, key)` 创建 PaymentIntent；`charge` 用已有支付方式发起离线确认；`confirm_payment` 确认已有 Intent。金额是币种最小单位的正整数，例如 USD 的美分或 JPY 的日元；不使用浮点。返回 `requires_action`、`requires_payment_method` 或 `processing` 不代表支付成功，应按返回状态完成 Stripe.js 认证并等待 Webhook。退款使用 `refund(payment_intent_id, options, key)`，支持部分金额；退款受理不代表资金已退回。

`invoices(parameters)` 保留供应商分页响应，支持 `limit`／`starting_after`；`find_invoice`、`invoice_document` 返回归属该客户的账单和厂商 PDF／托管链接。`invoice_item`、`create_invoice`、`finalize_invoice`、`pay_invoice`、`void_invoice` 支持手动账单流程，`preview_invoice` 和订阅的 `preview` 用于变更预览。`local_invoices` 返回 Webhook 同步的精简投影。

`tax_ids`、`create_tax_id`、`delete_tax_id` 管理税号；`balance` 返回整数最小单位的未格式化余额，`balance_transactions` 查询流水，`apply_balance` 添加贷记或借记。`report_usage(event_name, value, identifier, timestamp, key)` 使用 Stripe Billing Meter Events；事件标识符需稳定且唯一。税率、自动税、折扣、计费锚点等供应商参数可通过 `with_options`、`update`、Checkout／账单 options 传入。options 仅接受应用构造的服务端参数，不能直接使用整份 HTTP 请求。

## Webhook 与事务 {#webhooks}

为每个具名账户创建固定 HTTPS Webhook 端点。Stripe 订阅 `customer.subscription.*`、`invoice.*`，并按业务添加 PaymentIntent、退款等事件。路由应排除仅适用于浏览器表单的 CSRF 校验，依靠签名验证。必须传入未经 JSON 重编码的原始请求字节：

```rust
let event = client.verify_webhook(&headers, &raw_body)?;
store.process(&client, &event, |tx, event| Box::pin(async move {
    // 在 tx 中更新业务权限、订单或事务发件箱。
    // event.currentSubscription()／currentInvoice() 是本次核对的当前投影。
    // event.data() 是原始事件，可能迟到；权限变更应依据当前投影。
    let _ = (tx, event);
    Ok(())
})).await?;
let response = event.success(); // 仅在事务提交或重复确认后返回 HTTP 200。
```

`handle_webhook` 是不添加应用 SQL 的同步入口。`process` 将事件去重、订阅／账单投影、业务 SQL 与完成标记放进同一事务；异常或取消时回滚，厂商重发可恢复。同一事件 ID 换内容返回 `EventConflict`。客户锁下读取当前供应商对象，避免迟到通知把新状态覆盖成旧状态；此过程需要可用的供应商 API，失败时返回失败以便重发。

未关联的订阅／账单客户会被拒绝。先建立可信客户映射，再开放 Checkout；从 Dashboard 创建的订阅需使用已关联客户，名称可放在 `metadata.cashier_type`，缺省为 `default`。其他已验签事件可由处理器消费，包不擅自把它们映射为付款或履约完成。只做事务内 SQL；发货、邮件等外部操作通过应用事务发件箱完成。不要在处理器内反向发起计费变更或嵌套处理同一事件。

Stripe `customer.deleted` 会向供应商核实，在同一事务停用客户映射并取消本地订阅投影；处理器通过 `customer_deleted()` 撤销业务权限。删除后如需重新关联，必须显式验证新客户并调用 `attach_customer`；旧客户通知继续拒绝。

Stripe HMAC 时间窗口 300 秒，拒绝未来超窗、重复签名头、跨模式以及 Connect 账户事件；账户轮换可配置多个可信密钥。本包支持 Stripe 直接账户；Connect 平台账户分账与代商户授权需要专用集成。

## 重试与原生 API {#errors-and-api}

所有变更使用稳定业务操作键，Stripe 将账户与操作键的稳定哈希作为 `Idempotency-Key`。库不自动重试；网络失败、超时、重定向、5xx、429 或无法解析的成功响应返回 `OutcomeUnknown`，应复用原键查询／核对已有操作。其他明确 HTTP 拒绝返回 `Rejected(status)`，错误不包含密钥或供应商响应正文。

HTTP 请求禁用重定向，超时 15 秒，响应上限 1 MiB；观察记录中移除正文、授权头与原始业务操作键。`Client::api(method, path, parameters, key)` 提供通用服务端 REST 入口：Stripe 使用 `/v1/...` 与表单编码，GET 参数编码进查询，保留供应商分页结构。该高级入口不执行本地客户归属检查或自动同步，应用必须自行校验；日常用户操作优先使用绑定身份的 `Billable`。

本包不提供框架PHP Trait、Blade 组件或逐方法 PHP 兼容层。前端使用厂商托管页面／官方 JS SDK，PDF 使用厂商账单链接。生产接入还需有效商户、产品权限、价格目录及可访问的回调地址。
