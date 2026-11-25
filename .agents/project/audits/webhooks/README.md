# Webhook 收发扩展实施与验收

## 当前边界

2026-11-25 用户启用持续开发目标，按已核定[计划](../../plans/webhooks.md)实现两个独立可选包。此前“仅方案、不实现”的授权边界已被本次明确开发授权替代；不授权推送、正式部署、发布或付费操作。持续目标未完成，WH-00至WH-07均未整体闭合。

首批实现两包的配置／Provider、签名／退避及外部扩展基础；后续[接收记录批次](receiver.md)补入手动 Processor、存储及模型委托。[公开路由／控制器与具名工厂](routes.md)已补齐本批入口和真实消费者；[载荷快照与数据库附件读取](payload.md)已接入。[原生处理 Job](jobs.md)、[同库原子入队](atomic-enqueue.md)及[接收 Worker 租约](processing-lease.md)已接入。已补[签名事件 ID 去重](verified-event-deduplication.md)与[同库待关联延期](deferred-processing.md)，以及[领取尝试和旧 Worker 写回隔离](processing-attempts.md)、[关联就绪主动唤醒](deferred-wakeup.md)和[有限摘要窗口去重](body-deduplication.md)。[接收保留期清理](retention.md)与[无签名查单提示](unverified-hints.md)已接入。[有限验签判定依据](verification-evidence.md)已提供按分钟审计与范围清理。[私有 Storage 附件引用](attachment-storage.md)和[过期对象清理及暂存补偿](attachment-cleanup.md)已接入。已接入[接收记录提交后的持久派发恢复](dispatch-recovery.md)。已接入[入列后丢失任务的有界补派](processing-recovery.md)。已接入[跨队列后端的持久业务延期及唤醒](durable-deferred-recovery.md)。已接入[耗尽诊断与受控接收重放](manual-replay.md)。已接入[发送 Builder 快照与 HTTP 固定目的地](send-builder.md)。已接入[持久领取与首次安全投递](first-delivery.md)。已接入 [Builder 派发和发送事件](dispatch-events.md)。[出站定时恢复](outbound-recovery.md)已接入。[具名发送 Job 扩展](custom-job.md)已接入。[出站人工重放](outbound-replay.md)已接入。[端点当前版本许可](endpoints.md)已接入。[出站共享限额](delivery-limits.md)已接入。[订阅有界扇出](subscription-fanout.md)及[出站 TLS 与客户端身份](outbound-tls.md)已接入。[出站具名 SOCKS5](outbound-proxy.md)已接入，[出站诊断](outbound-diagnostics.md)及[出站快照与尝试留存](outbound-retention.md)已接入。[共享发布正文回收](publication-retention.md)已接入。普通 CONNECT 代理、幂等墓碑回收及部署拓扑仍待完成。**不以验签返回200冒充可靠接收，不以生成签名头冒充发送模块。**实际进度见[逐类型清单](contracts.json)；扫描清单包含固定源码直接声明及父类线索，另登记packageRegistered注册的Route::webhooks宏（方法白名单含query），不能证明所有继承契约已完成。后续必须补齐构造／字段／trait／模型继承裁决与实际消费者。

## 固定来源与采用理由

- Spatie Client：`ff886d1bae33d35ad98b6c91403f280df158129d`，Server：`6da6be3a721190bcdedcdef32d31d5980b77eb0e`。固定源码17＋17个类型、配置与测试已下载至项目批次目录；MIT原文随包保留。参考其类职责、默认值与公开方法，不运行或分发PHP。
- 默认验证器／签名器采用原始正文的HMAC-SHA256小写十六进制；URL／时间戳不参与。直接复用项目已有aws-lc-rs原语与恒定时间验证，不另造加密库。默认签名器的头名称Getter保留`Signature`及自定义原大小写，HTTP HeaderMap按原生语义归一化。
- ConfigRepository按名称后值覆盖、未知返回None；Provider按源码使用scoped绑定，跨HTTP／命令生命周期不串运行时追加；已取得Arc保持原快照。并发应用互相隔离。源码中WebhookConfig构造校验和模型／Job等完整元数据尚待下一批连接，不能把本批部分配置入口算作该类型完整对齐。
- Standard Webhooks直接复用crates.io **standardwebhooks=1.3.0**，发布源码提交`daee8284cfe9d4e435ec16e3e68a8f30084d6aaf`，checksum `ca0476284408cba80764ef46b87a479225c97b4171b59e7eb89244c7f7cf8c9d`。该发布物与计划调研GitHub提交`4a8ee40c5bcb6157f856905cc8871040d41a78ce`不同，后者已有额外非对称代码，**不假称二者相同**。本批使用发布物已提供的对称签名／验证，不移植整个Svix服务。
- 锁文件只新增两个registry包：standardwebhooks1.3.0（MIT）与hmac-sha2561.1.15（ISC），其他依赖复用现有锁定版本。SDK不声明MSRV，实际Rust1.94.0编译、运行和严格Clippy通过；time0.3.55声明1.88，base640.23.1声明1.71。原文许可及来源位于两个包的`LICENSES`／`THIRD_PARTY_NOTICES.md`，版本、checksum与许可文件摘要见[依赖固定值](dependency-pins.json)，可执行`python3 -B tests/webhooks_licenses.py`。此检查限本次依赖及随源码保留的许可，不冒充正式制品发行或商业合同审批。
- 已查同机`openrust-v0.x/vendor/openrust`，没有可复用的独立通用Webhook包；现有Cashier专用验签及Pay同一事务通知仍归领域包，不拆散或强制迁移。复用核心Provider、包发现、配置、HTTP、Queue与队列化Artisan，不新增队列／HTTP引擎或共享crate。

固定源码入口：[Client ConfigRepository](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookConfigRepository.php)、[Client Provider](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/WebhookClientServiceProvider.php)、[Default Validator](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/src/SignatureValidator/DefaultSignatureValidator.php)、[Server Signer](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/Signer/DefaultSigner.php)、[Backoff](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/BackoffStrategy/ExponentialBackoffStrategy.php)、[SDK发布源码](https://github.com/standard-webhooks/standard-webhooks/tree/daee8284cfe9d4e435ec16e3e68a8f30084d6aaf/libraries/rust)。未执行上游PHP／Composer测试。

## Rust适配与安全边界

- 使用原生snake_case／UpperCamelCase。验证、Profile和响应返回Result；原请求变为保留原字节／方法／头与原始URI的`WebhookRequest`（`with_uri`保留查询编码），调用方先限制读取流，不能先无限收集再期待快照替其限内存。Signer使用不可序列化、无Debug的借用`SigningContext`，显式提供正文／URL／密钥／ID／时间，支持Standard协议附加头。
- 配置缓存只存`{"env":"变量名"}`引用，按应用的不可变Environment快照解析。拒绝把明文signing_secret字符串放进包配置；直接运行时注入使用`WebhookConfig::new`，密钥私有／Zeroizing／Debug脱敏。缺失引用返回配置错误，不把异常降级成无签名提示。历史凭据修订跨重启保留尚未实现，revision标签不能冒充持久修订库。此项是明确安全适配，区别于上游把明文secret存进配置数组。
- DefaultValidator保留缺签名与空密钥的判定顺序，包括PHP中字符串`0`的空值边界；配置错误先于非空坏签名格式拒绝。重复签名头按安全增强拒绝。轮换最多总共8个同配置密钥，不跨账号尝试。未签名Hint／可信类型与事件派发待WH-02／WH-03，不在本批新增放行旁路。
- SDK保留5分钟窗口与同一头中的多签名；拒绝重复协议头、过长／空ID。用i128先界定时间窗，并检查now加减300安全，避免发布库在i64极值时间运算溢出。发送headers拒绝无法稳定通过HTTP解析的ID（非ASCII、控制、首尾空白）及超过256字节。SDK原样要求UTF-8正文；Spatie默认算法支持原始二进制。
- 默认正文1MiB、可配置最多16MiB；头值最多128条／名称和值总计64KiB；配置来源最多128项、名字与revision最多128字节、单个密钥8192字节。预算及更低传输层限制已写三语；不为兼容去掉边界。
- 退避按固定源码：0→1，1→10，2→100，3→1000，4→10000，>4→100000秒。Rust使用非负整数；实际第一次尝试为1。它尚未接入投递预算／Queue重试，不把纯退避数值测通算成重试状态机通过。

## 首批代码与实际消费者

- 可选包路径`vendor/openrust/webhook-client`／`webhook-server`，根Cargo只增加显式feature／可选依赖；默认应用、Provider、路由和config目录没有预装扩展。发布模板分别是`config/webhook_client.rs`／`webhook_server.rs`，配置命名空间保留`webhook-client`／`webhook-server`；用户显式接入config/mod.rs后编译，重复发布保留定制。
- 外部crate测试实现Validator、Profile、Response、Signer及Backoff；默认Profile／响应与自定义结果均直接执行。配置仓库替换／旧快照、应用与生命周期隔离、具名重复配置、凭据引用、失败路径均有消费者。
- `tests/support/webhooks/app.rs`是**测试专用应用**：包发现正常装配，HTTP流有界收集后调用验证与响应；CLI签名命令、队列化命令和结果文件也只在夹具中。403／404／413是该夹具应用的决定，不冒充尚未实现的WebhookController默认错误契约。只证明公共基础组件可被HTTP／Artisan／Worker使用，不证明已持久接收、可靠发送或集群。
- `tests/application_skeleton.rs`补上kebab-case feature到snake_case配置文件／模块的检查，防止发布模板重新混进默认骨架。三语正文六段Rust示例由`tests/webhooks_docs.py`提取后真实编译，行为另由真实消费者验证。

## 首批验收

初始归档基线`4d45b5609d92b42303530869d319924ccb29a393`，集成归档基线`5b159fff7fb0a27d3dbfb16e9d426eda35553c98`，只覆盖本任务文件，不带入其他对话WIP。工作树另出现new-relic包及其Cargo清单与测试入口，属其他对话在途，本批仅按hunk提交Webhook清单，不覆盖或顺带提交。源码／日志`target/tmp/webhooks-20261125`。同一持续目标顺序复用已结束上一批的独占`target/tmp/driver-extensions/cargo-target`，不复制缓存。构建、测试及子进程均经build_guard，Rust1.94.0、locked，正常增量与并发。

- 包自身12项：Client契约6＋Provider1，Server契约4＋Provider1。包含8种原始字节向量的多组正反例、官方Standard向量／多签名／时间窗／极值、重复头、原始大小写、缺密钥／`0`／非ASCII签名头、轮换、外部实现和生命周期。
- 三语6项编译消费者＋骨架2项。workspace同名contracts／provider父消费者亦执行；其中Telescope专用Redis命令用例标记ignored，未提供其TELESCOPE_TEST_REDIS_URL，不能计为本批已验。未要求或宣称全workspace业务测试全通过。
- `tests/webhooks_features.py`从隔离源分别构建关闭、Client-only、Server-only、Both四种正式main。真实package:discover、config:show、显式发布通过；关闭的包无配置，未发布前无应用模板。
- `tests/webhooks_acceptance.py`：真实HTTP共50个请求，缓存前后分别覆盖原字节／改字节拒绝、JSON／form重复字段／multipart／二进制、重复协议头拒绝、两协议由独立Python HMAC核对Artisan签名、过期／无签名／未知名／超限；包发现、重复发布保留注释定制、缓存JSON及config:show无实际密钥、新进程按引用读取自己的轮换密钥。
- `tests/webhooks_worker_acceptance.py`：PostgreSQL18.7、MySQL8.4.13、SQLite分别实际迁移与config:cache，使用既有database队列化Artisan命令，独立Worker执行两协议签名；派发端假凭据不入队，Worker用自身凭据，jobs／failed_jobs均为0。本批未改Queue／Cache，不以此冒充完整重试、跨节点领取或Redis专项。
- workspace/all-targets＋两feature严格Clippy、全仓fmt、许可检查通过。三语内容／命令门禁与站点构建、12个Webhook页面、12个通用桌面／移动端、根／子目录全导航及21个明确支持范围页面检查通过。站点保留已有大块体积提示，不调整阈值隐藏。

失败与修正保留：首次HTTP脚本默认表单Content-Type却传非UTF-8二进制，得到400；改为相符的octet-stream并另加真正JSON／form／multipart用例后通过。新增外部Response测试最初把str当Body，按真实公开签名修正为into。文档曾误用工作区入口构建，该日志标记not-acceptance，交付以独立快照重建结果为准。相关原日志保留；不把首次失败抹掉。

## 下一批与仍缺能力

继续WH-00构造／字段／继承逐项裁决与WH-01完整安装装配；注意默认签名不保护Webhook-Id、Timestamp或URL，不能直接把这些未签字段当可信事件身份；无ID摘要策略须覆盖真实业务输入并有窗口，不能误合并GET／query的不同请求。手动Processor、原文接收记录与模型委托storage已在[后续批次](receiver.md)验收，[公开控制器／路由及注册工厂](routes.md)已有实际验收，继续核对WH-02事件／异常契约，附件本地清理已验，远端及跨主机仍待验。随后WH-03补齐领域幂等消费者和双拓扑，再进入WH-04首次安全发送、WH-05持久发送、WH-06运维及WH-07全门禁。全新生成应用／完整安装模板、正式制品许可收集、持久凭据修订也须在各自门禁实测，不因本批功能开关通过而关闭。

WH-V01至WH-V27仍按计划逐项完成；本批只有部分基础证据。真实远程多主机、Linux／Windows、生产TLS／代理／mTLS及供应商／平台未验，模拟拓扑也尚未执行。不推送／不发布／不操作正式环境，目标保持进行中。
