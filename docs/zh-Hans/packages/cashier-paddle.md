# Cashier（Paddle）

## 安装与配置 {#configuration}

`openrust-cashier` 集成 Paddle Billing API v1，支持客户、地址、Checkout 交易、订阅变更、账单、退款与持久 Webhook。与 Stripe 共用 `Billable`、`BillingStore` 和包 Provider；安装、五张表及通用事务约定见[Cashier（Stripe）](billing.md)。Paddle Classic 不适用。

需要定制编译配置时，运行 `artisan vendor:publish --tag=cashier-config` 发布 `config/cashier.rs`。重复发布保留应用修改，只有 `--force` 才覆盖。在应用 `config/mod.rs` 声明 `mod cashier;`，并在配置组中加入 `("cashier".into(), cashier::load(env)?)`，然后重新编译应用。不发布配置也可使用包内环境默认值。

```dotenv
CASHIER_DRIVER=paddle
PADDLE_SANDBOX=true
PADDLE_API_KEY=pdl_sdbx_apikey_...
PADDLE_WEBHOOK_SECRET=pdl_ntfset_...
```

`PADDLE_SANDBOX` 按 Cashier Paddle 默认 `false`，沙箱凭据应显式设置 `true`；`PADDLE_AUTH_CODE` 优先于 `PADDLE_API_KEY`。显式设置的 `CASHIER_LIVE` 保留旧模式覆盖，具名账户直接配置 `live`。`createAsCustomer(options, key)` 复用已绑定客户，新客户要求显式邮箱；返回供应商 JSON 并保存身份映射，不模拟 PHP morphOne，也不按邮箱自动关联已有客户。关联已有账户使用经过身份校验的 `attachCustomer`。API 与 Webhook 密钥仅保存在服务端。

## Checkout 与试用 {#checkout}

```rust
use openrust_cashier::{Billable, BillingStore, Price, facades::Cashier};
use serde_json::json;

let client = Cashier::driver("paddle")?;
let store = BillingStore::new(connection.clone());
let user = Billable::new(client.clone(), store.clone(), "users:42")?;
user.createAsCustomer(json!({"email":"buyer@example.com"}), "customer:42").await?;
let transaction = user.newSubscription("default", vec![Price::new("pri_monthly", 1)?])?
    .withCoupon("dsc_welcome")?
    .checkout(json!({}), "subscription:42:first").await?;
```

返回 Paddle 交易对象。将 `id` 交给已初始化的 Paddle.js `Paddle.Checkout.open({transactionId: id})`，或使用交易中的 `checkout.url`。默认付款链接需已获准并集成 Paddle.js。浏览器完成事件不授予会员权限，等待 Webhook 与业务事务提交。

Paddle 通过交易完成创建订阅，不提供 Stripe 式直接创建订阅 API；`newSubscription(...).create(...)` 明确返回 `Unsupported`。试用周期在 Paddle 价格目录配置，`trialDays`／`trialUntil` 不会伪造试用；开始收费可调用 `activate`。`withCoupon` 使用 Paddle discount ID，名称存入 `custom_data.cashier_type`。一次性价格可通过 `user.checkout(...)` 销售。

## 订阅、账单与客户 {#subscriptions}

`subscription`、`subscriptionById`、`syncSubscription` 与状态判断沿用[通用订阅接口](billing.md#subscriptions)。订阅 Webhook 同步客户和多个价格／数量，未关联客户拒绝处理。

- `swap(prices, proration, key)`、`updateQuantity` 更新完整价格集合。`Prorate` 计入下一账期，`Invoice` 立即按比例计费，`None` 不计费。
- `cancel` 在下次计费期取消，`cancelNow` 立即取消；`resume` 清除未执行的取消／暂停计划，或恢复已暂停订阅。已经完成的取消不能恢复，应重新 Checkout。
- `pause(now, until, key)` 可在当前或下一计费期暂停，可选 RFC3339 自动恢复时间；`activate` 提前结束试用。
- `charge(prices, key)` 对订阅添加一次性收费；`preview(options, key)` 预览变更。`update` 与 `asProviderSubscription` 提供核对归属后的供应商参数／对象。
- `paymentMethodUpdateTransaction()` 获取更改付款方式的交易，交给 Paddle.js；`billingPortal` 创建客户门户会话。

`addresses`／`createAddress` 管理账单地址；`updateCustomer` 更新客户。`transactions(parameters)` / `invoices(parameters)` 返回原生分页响应，使用 `per_page`／`after`，`findInvoice` 获取交易，`invoiceDocument` 获取 Paddle 托管账单链接。`refund(transaction_id, options, key)` 创建退款 adjustment，options 填写 `type`、`reason` 和部分退款的 `items`。退款可能处于审批中，不能把受理响应当成退款到账。

Paddle 负责税费与结算；Stripe 的 SetupIntent、卡片绑定、税号、余额流水、PaymentIntent 与 Meter Events 在 Paddle 上明确返回 `Unsupported`。供应商专属能力使用 Paddle 对应接口，不能把两家参数混传。

## Webhook 与重试 {#webhooks}

Paddle 通知目标订阅 `subscription.*`、`transaction.*` 及业务所需的 adjustment 等事件。使用该账户的 `Client::verifyWebhook` 检查原始请求，随后调用 `BillingStore::process` 或 `handleWebhook`，事务提交后才返回 `event.success()`。签名使用 `Paddle-Signature`、原始字节 HMAC 与五秒时间窗口，支持多个 `h1` 和可信密钥轮换。不要用事件的 `occurred_at` 代替本次投递签名时间。

包自动同步订阅及交易的精简投影；其他已验证事件交给应用 SQL 处理器。通知去重以 Paddle `event_id` 为准。旧事件到达时在客户锁下拉取当前 API 对象，防止本地状态倒退。详细回滚、取消恢复、业务身份和事务发件箱约定见[Webhook 与事务](billing.md#webhooks)。

Paddle 不承诺 Stripe 式幂等键。所有高层变更先在 `cashier_operations` 持久认领业务操作键，再发送一次；同键再次调用不会发送，即使上次进程中断或已返回成功。`operationStatus(client, key)` 返回 `pending`／`done`／`rejected`／`unknown`；返回结果未知时，先在 Paddle 通过原交易／订阅／客户标识核对。

同键已执行返回 `OutcomeUnknown`，同键改参数返回 `EventConflict`。核对后才能决定是否使用新的业务键；不要通过换键盲目补发。客户创建结果未知时须先查询 Paddle 账户，验证归属后 `attachCustomer`。操作记录的保留周期必须覆盖业务可能重试的时间，不要清空后重放历史操作。

`Client::api` 可调用 Paddle REST API；它保留原生响应封装，使用 `/subscriptions/...` 等路径、JSON 请求与 `Paddle-Version:1`。该高级入口绕过持久操作认领和客户归属检查；如需防重，使用 `BillingStore::execute`。库不自动跟随响应中的分页 URL，应用提取经过认可的游标再请求固定 API 端点。
