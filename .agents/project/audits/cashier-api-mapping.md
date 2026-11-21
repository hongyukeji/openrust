# Cashier 已声明接口对照（2026-11-21）

固定来源：Stripe `d5c015490ff77f2c138fe79b86d2a982787fe3ac`，Paddle `82bfc68eb72c38e666a4900baeecca3621840259`。联网读取两个固定归档的 src/、原生配置和许可证。下表逐个列出当前 Billable/SubscriptionBuilder/SubscriptionHandle 的公开方法，列出原文定义位置；同名不代表签名或返回类型相同，具体适配见表后说明。

| Rust 对象／方法 | Stripe 原文 | Paddle 原文 |
|---|---|---|
| `Billable::new` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::client` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::customerId` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::attachCustomer` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::stripeId` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::hasStripeId` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::createAsStripeCustomer` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::asStripeCustomer` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::createOrGetStripeCustomer` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::updateStripeCustomer` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::updateOrCreateStripeCustomer` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::createAsCustomer` | Rust 适配入口／无直接同名 | `src/Concerns/ManagesCustomer.php` |
| `Billable::billingPortalUrl` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::redirectToBillingPortal` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::createOrGetCustomer` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::updateCustomer` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::billingPortal` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::newSubscription` | `src/Concerns/ManagesSubscriptions.php` | `src/Concerns/PerformsCharges.php` |
| `Billable::subscription` | `src/Concerns/ManagesSubscriptions.php` | `src/Concerns/ManagesSubscriptions.php` |
| `Billable::subscriptionById` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::syncSubscription` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::subscribed` | `src/Concerns/ManagesSubscriptions.php` | `src/Concerns/ManagesSubscriptions.php` |
| `Billable::checkout` | `src/Concerns/PerformsCharges.php` | `src/Concerns/PerformsCharges.php` |
| `Billable::createSetupIntent` | `src/Concerns/ManagesPaymentMethods.php` | Rust 适配入口／无直接同名 |
| `Billable::paymentMethods` | `src/Concerns/ManagesPaymentMethods.php` | Rust 适配入口／无直接同名 |
| `Billable::addPaymentMethod` | `src/Concerns/ManagesPaymentMethods.php` | Rust 适配入口／无直接同名 |
| `Billable::updateDefaultPaymentMethod` | `src/Concerns/ManagesPaymentMethods.php` | Rust 适配入口／无直接同名 |
| `Billable::deletePaymentMethod` | `src/Concerns/ManagesPaymentMethods.php` | Rust 适配入口／无直接同名 |
| `Billable::defaultPaymentMethod` | `src/Concerns/ManagesPaymentMethods.php` | Rust 适配入口／无直接同名 |
| `Billable::pay` | `src/Concerns/PerformsCharges.php` | Rust 适配入口／无直接同名 |
| `Billable::charge` | `src/Concerns/PerformsCharges.php` | `src/Concerns/PerformsCharges.php` |
| `Billable::confirmPayment` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::refund` | `src/Concerns/PerformsCharges.php` | Rust 适配入口／无直接同名 |
| `Billable::invoices` | `src/Concerns/ManagesInvoices.php` | Rust 适配入口／无直接同名 |
| `Billable::transactions` | Rust 适配入口／无直接同名 | `src/Concerns/ManagesTransactions.php` |
| `Billable::findTransaction` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::localInvoices` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::findInvoice` | `src/Concerns/ManagesInvoices.php` | Rust 适配入口／无直接同名 |
| `Billable::invoiceDocument` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::previewInvoice` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::createInvoice` | `src/Concerns/ManagesInvoices.php` | Rust 适配入口／无直接同名 |
| `Billable::invoiceItem` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::payInvoice` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::finalizeInvoice` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::voidInvoice` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::taxIds` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::createTaxId` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::deleteTaxId` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::rawBalance` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::balance` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::balanceTransactions` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::applyBalance` | `src/Concerns/ManagesCustomer.php` | Rust 适配入口／无直接同名 |
| `Billable::reportUsage` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::addresses` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `Billable::createAddress` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::trialDays` | `src/SubscriptionBuilder.php` | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::trialUntil` | `src/SubscriptionBuilder.php` | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::skipTrial` | `src/SubscriptionBuilder.php` | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::anchorBillingCycleOn` | `src/SubscriptionBuilder.php` | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::withMetadata` | `src/SubscriptionBuilder.php` | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::quantity` | `src/SubscriptionBuilder.php` | `src/SubscriptionBuilder.php` |
| `SubscriptionBuilder::withOptions` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::withCoupon` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::create` | `src/SubscriptionBuilder.php` | Rust 适配入口／无直接同名 |
| `SubscriptionBuilder::checkout` | `src/SubscriptionBuilder.php` | `src/SubscriptionBuilder.php` |
| `SubscriptionHandle::asStripeSubscription` | `src/Subscription.php` | Rust 适配入口／无直接同名 |
| `SubscriptionHandle::asPaddleSubscription` | Rust 适配入口／无直接同名 | `src/Subscription.php` |
| `SubscriptionHandle::asProviderSubscription` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionHandle::paymentMethodUpdateTransaction` | Rust 适配入口／无直接同名 | `src/Subscription.php` |
| `SubscriptionHandle::refresh` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionHandle::cancel` | `src/Subscription.php` | `src/Subscription.php` |
| `SubscriptionHandle::cancelNow` | `src/Subscription.php` | `src/Subscription.php` |
| `SubscriptionHandle::resume` | `src/Subscription.php` | `src/Subscription.php` |
| `SubscriptionHandle::pause` | Rust 适配入口／无直接同名 | `src/Subscription.php` |
| `SubscriptionHandle::charge` | Rust 适配入口／无直接同名 | `src/Subscription.php` |
| `SubscriptionHandle::pauseCollection` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionHandle::resumeCollection` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionHandle::activate` | Rust 适配入口／无直接同名 | `src/Subscription.php` |
| `SubscriptionHandle::extendTrial` | `src/Subscription.php` | `src/Subscription.php` |
| `SubscriptionHandle::swap` | `src/Subscription.php` | `src/Subscription.php` |
| `SubscriptionHandle::updateQuantity` | `src/Subscription.php` | `src/Subscription.php` |
| `SubscriptionHandle::preview` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |
| `SubscriptionHandle::update` | Rust 适配入口／无直接同名 | Rust 适配入口／无直接同名 |

## 模型和返回值

- Billable：PHP model trait 对应绑定可信 owner/client/store 的 Rust 对象；应用字段通过 options 显式传入，不反射模型。Stripe 客户接口补全原名，createAsStripeCustomer 的已有客户错误独立为 CustomerAlreadyCreated；createOrGetStripeCustomer 保留 expand；Portal URL 与 302 响应分别返回。Stripe 方法在请求和数据库操作前拒绝 Paddle。
- Paddle createAsCustomer 保留“已有关联则复用”的分支，新建要求 email；当前返回 JSON 与本地 owner 映射，不实现 PHP Customer morphOne、trial_ends_at 本地模型关系，也不按邮箱自动关联旧客户。这是显式身份归属策略，非 Rust 必然限制；已在三语正文说明，不冒充完全等价。
- SubscriptionHandle：对应上游 Subscription 的活动操作；Deref 到本地 Subscription 快照以读取状态。Subscription/SubscriptionItem/Invoice 保留类型化状态字段，与厂商 SDK 的对象类型不同。补原名 asStripeSubscription/asPaddleSubscription，并拒绝另一供应商。无须再复制存储或客户端。
- 客户、付款方式、PaymentIntent、Checkout、外部账单等厂商响应为 serde_json::Value；本地 Invoice/Subscription 为持久化记录。当前不提供 Stripe SDK Customer/Payment/Collection 类型、Blade、PDF 渲染器和任意 Eloquent 关系。
- rawBalance 返回 Stripe 整数最小单位；原有 balance 是同一整数便利入口，明确不同于 PHP balance 的货币格式化字符串。billingPortal 返回会话 JSON，billingPortalUrl 返回 HTTPS String，redirectToBillingPortal 返回原生 HTTP 302。
- 参数必须显式传入；供应商修改操作保留稳定业务 key、Result、异步和所有权检查。其余同名方法在原有协议测试中按具体载荷验收，不据名称声称 PHP 可选参数、SDK 类型及全部上游行为相同。

## 配置

| 来源 | 当前入口与默认值 |
|---|---|
| Stripe key/secret/webhook.secret | cashier.key / cashier.secret / cashier.webhook.secret，STRIPE_KEY / STRIPE_SECRET / STRIPE_WEBHOOK_SECRET |
| Paddle api_key/webhook_secret/sandbox | cashier.paddle.api_key / webhook_secret / sandbox；PADDLE_AUTH_CODE 优先于 PADDLE_API_KEY，PADDLE_SANDBOX 默认 false |
| 两家独立 PHP 包的同名 cashier 分组 | 单一 Rust 包保留 Stripe 顶层键，Paddle 单独子空间，避免同名键冲突；不是语言强制拆分，属于显式装配取舍 |
| 原有具名账户 | cashier.default / drivers；匹配的 profile 整体优先，已有账户作用域不改变，CASHIER_LIVE 显式存在时保留旧模式覆盖 |
| 未消费的上游配置 | path/webhook routes、currency locale、invoice renderer、seller/client-side token 等未实现功能不预设无效配置；相应浏览器 JS/路由由应用按正文装配，不宣称包默认行为等价 |

配置缓存继续复用已有 Provider 的 configurationIsCached 分支，缓存命中不读取环境。API 版本仍按固定官方 stripe-php；候选库取舍见 [扩展复用决定](../plans/extension-reuse-decisions.md)。

本记录是已交付接口的逐项映射，不是全量上游 API 完成声明。验收结果回填全量扩展审计 R2。
