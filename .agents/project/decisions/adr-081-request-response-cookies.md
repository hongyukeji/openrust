# ADR-081：请求 Cookie 与响应队列

- 状态：已接受，C04-C 子簇已验收
- 日期：2026-10-29
- 对应：L019.03、L020；不关闭完整Session、flash、CSRF或FormRequest

## 背景

Laravel优先的Cookie行为需要贯通请求读取、响应Cookie、队列和加密。现有Session/auth读取Cookie header，tower中间件会重建Cookie扩展；只增加一个解密bag无法让真实会话使用同一视图。现有Encryptor和Session store保留，不建立平行内核。

## 决策

1. 创建不可变Cookie factory默认与请求隔离的可变队列。公开创建、forever、forget、queue/queued/unqueue及响应组合入口；name+path为queue身份，domain不参与，覆盖原path不改变其插入次序；不同path并存。取消/超时后随请求drop释放，不spawn后台队列。
2. RequestInfo与Request读取同一Cookie bag：直接key而非dot路径，全bag入口独立提供；null/缺失采用default，空串仍存在。Cookie不参与普通输入trim/null规范化。
3. wire解析单独对应PHP SAPI：分号分隔、只对值raw URL decode（加号不转空格）、名称不URL decode、点/空格规范化、括号数组；顶层普通名重复保首项，嵌套叶按原注册规则分别验收。Symfony显式cookies数组与wire解析分层，不拿parse_str或B1表单parser冒充Cookie行为。
4. 外层Cookie middleware提供唯一明文header视图供现Session/auth，另保有界原始wire元数据供公开检查与严格重复凭据拒绝。不能因bag选首项或重编码header绕过原认证安全校验。队列先附加响应，再加密全部应受保护Cookie；保留原属性和现Response/Json/envelope路径。
5. 加密复用现Encryptor与本地key ring：旧key只解密，当前key写新密文；except公开配置按各阶段名称精确匹配：入站为SAPI解析后的根键，出站为Cookie字面名称（固定EncryptCookies的`decrypt`与`encrypt`内各自调用`isDisabled`）；无括号普通名可双向绕过，`prefs[color]`不因排除`prefs`而跳过出站加密，不擅自新增根名归并；认证/加密失败不能降级为明文。可选加密feature的边界显式报告，未安装保护不冒称已加密。
6. 无PHP应用数据或密文字节互通义务。沿已有or1 AEAD，以专用Cookie域AAD和认证payload中的完整叶名实现PHP名称前缀的可观察保护；数组叶名如root[nested][index]。解密认证失败与成功解密后的名称不匹配须分类，分别对应整根null与仅叶null且保形状；只把名称放AAD会混同二者，不采用该简化。
7. 应用参数错误安全失败；请求与响应总量、字段、树深和输出数量有界，超限不截断。UTF-8/类型表示、公开方法签名与确切限额在源码落地时就地补入本ADR与用户文档，不将普通cookie参数错误泄漏为panic或回显敏感值。

## 已定接线与表示边界

- `CookieServiceProvider::new()`要求加密feature和已注册Encryptor，缺失时启动明确报错；`plain()`仅供宿主显式选择明文的API/原生Cookie或测试路径，不是加密缺失时的自动降级。当前API宿主的显式选择不能冒称已实现Laravel默认web中间件组；完整web默认仍在C04余项。
- `build_router`在Provider贡献的HTTP层之外、现请求scope之内统一安装Cookie层，保证Session消费明文视图且同请求Jar共享；顺序不依赖Cookie/Session Provider登记巧合。仍由Provider显式选择策略，未登记不暗装保护，不建立第二生命周期内核。
- 公开Cookie响应组合器在Response extensions保留Cookie对象，再附加队列和加密；不能先序列化再读回，把原值中的分号等编码字符错误当明文加密。现Session原生Set-Cookie经统一适配保模型属性。
- 多个Cookie header在Rust按HTTP crumbs分号语义解析；真实PHP内置服务器会先以逗号合并，这属于transport差异而非PHP变量注册规则。参考样本独立分类，认证仍检查原始重复凭据，不能靠选择首项绕过。
- 入站UTF-8无法表示时安全400 `input_cookie_encoding_invalid`；树1024 entries/64层与总64KiB、响应128 Set-Cookie/64KiB预算显式拒绝413 `input_cookie_limits_exceeded`。PHP默认max_input_vars=1000的截断不是Rust拒绝策略，分列验收。非法应用参数为安全500 `cookie_parameters_invalid`，不回显敏感值。

Secure默认须核完整Laravel Router→Symfony Response::prepare链：HTTPS响应会为secure=null的Cookie补安全默认；不能仅看Cookie初值false。公开可信RequestSecurity元数据供原生TLS宿主设置并优先使用；否则仅既有trusted_proxy_ranges匹配peer时采X-Forwarded-Proto首物理值的首逗号token，trim/lower后https/on/ssl/1为真，其余为假。未受信代理头和客户端absolute URI不能声明连接安全；显式Cookie secure=false仍覆盖默认。不增加第二代理配置或与固定源不同的无必要逗号拒绝策略。

SessionConfig补path/domain，服务factory从唯一最终session区段读取path/domain/secure/same_site；with_factory显式覆盖优先，缺整个区段才取裸factory默认；未注册Session配置schema的独立Cookie路径中，区段存在时same_site缺键或null表示无SameSite属性，字符串"none"则为SameSite=None，不能用lax覆盖null；注册SessionProvider时仍受现SessionConfig字符串schema与默认值约束，不把该独立路径测试当成完整Session nullable配置已对齐。内部typed视图仅读取同一仓储，HTTP不反向依赖optional auth。旧Session secure=true布尔默认是现有事实，本簇不因此声称完整Session默认已与Laravel等价。

显式with_cookie_config在该Provider的boot写入同一session仓储，覆盖文件/环境；随后Provider的显式overlay仍遵通用先后顺序，SessionSettings和CookieFactory读取同一最终结果。with_factory表示宿主明确的Cookie-only覆盖。Session中间件在现RouterLayerRegistry装配阶段通过内部可失败factory读取最终配置，启动校验亦使用最终值；不保留即时boot时的陈旧副本，不新增生命周期或Hooks系统。

Expires解析沿固定Symfony：numeric先整数/有限浮点截断，非numeric按strtotime语义，解析失败为应用参数错误而非session cookie；成功时间戳<=0才表示session。标准HTTP-date保UTC快路径，相对日期复用现日历内核；公开from_header默认UTC/SystemClock，受管理路径提供最终应用CalendarContext。需读取clock一次并保持非mock语义，不能因测试FixedClock把strtotime行为变成Carbon test-now行为，也不改进程全局TZ。

## 固定依据与验收

固定framework为`d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`，docs为`960eedf0911155ad16326ade49c0ea5d18eda0b8`：InteractsWithInput、CookieJar、CookieServiceProvider、AddQueuedCookiesToResponse、EncryptCookies与CookieValuePrefix；wire规则另核PHP8.5.0 main/php_variables.c。与[ADR-017](adr-017-generic-session-runtime.md)既有Session和[ADR-034](adr-034-application-encryption-and-key-rotation.md)加密服务复用，输入职责沿[ADR-079](adr-079-request-input.md)。

验收须覆盖name/path顺序与覆盖、forget属性、读取默认/null/空串、真实wire与显式数组、并发隔离、取消、篡改/跨名/旧key、except与出站失败、queued/直接Cookie响应顺序，以及现认证重复凭据与Session双库回归。唯一modules/example、plain/auth生成项目、真实TCP和适用workspace门禁随簇交付；普通单测和固定PHP参考不能替代真实生产链路。
