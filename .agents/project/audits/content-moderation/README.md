# 内容审核工程核对

范围以[已确认开发计划](../../plans/content-moderation.md)为准。CM-00 起步核对公开契约与四家协议。下表为开发依据，运行验收随对应批次记录。

## CM-00 协议依据 {#protocols}

| 驱动 | 即时入口 | 异步入口与覆盖 |
| --- | --- | --- |
| 阿里云 | 国内站增强内容安全 `Green/2022-03-02`。`TextModeration` 以 `ServiceParameters` 中的原文调用，适用文档限定 600 字符，按实际服务代码和结果形态映射。`ImageModeration` 明确绑定 `Service`，用经授权的图片 URL 或 OSS 参数。两个入口分别核对响应字段，HTTP 成功后仍检查业务 `Code`。 [文本](https://help.aliyun.com/zh/document_detail/434034.html) · [图片](https://help.aliyun.com/zh/document_detail/2528742.html) | `VideoModeration` 提交，`VideoModerationResult` 以 `taskId` 查询。场景参数与帧、音频、OCR 的证据应逐项验收。查询的推荐间隔与 24 小时保留期用于确定轮询预算；到期或字段缺失无法产生 Allow。 [API 列表](https://help.aliyun.com/zh/document_detail/2528737.html) · [查询](https://next.api.aliyun.com/document/Green/2022-03-02/VideoModerationResult) |
| 腾讯云 | `2020-12-29` 标准内容安全，`tms.tencentcloudapi.com` 的 `TextModeration` 使用 UTF-8 Base64，官方限制 10,000 Unicode 字符，覆盖中文和英文。`ims.tencentcloudapi.com` 的 `ImageModeration` 使用标准场景，图片 OCR 单独核对。 [文本](https://cloud.tencent.com.cn/document/api/1124/51860) · [图片](https://cloud.tencent.cn/document/product/1124/137268) | 标准点播视频的 `CreateVideoModerationTask`／`DescribeTaskDetail`，按 `VIDEO` 与配置的 `BizType` 请求。视频 Agent 和生成内容识别使用同名 Action 但属于不同能力。以标准 API 查询补足回调中未覆盖的片段，并分别保留语音和画面结果。 [标准视频](https://cloud.tencent.com/document/product/1124/137270) · [API 列表](https://cloud.tencent.com/document/api/1265/80029) |
| Sightengine | `text/check.json`、`check.json`，分别选择文本规则／模型和图像模型。动图结果以采样覆盖记录。图片 OCR 与纯文本模型分别配置。 [API](https://sightengine.com/docs/reference) · [OCR](https://sightengine.com/docs/ocr-text-moderation-in-images-2.0) | `video/check.json` 提交、`video/byid.json` 查询／取消。账号级回调可使用 `Sightengine-Signature`，只对启用且实际签名的来源使用验签处理器。官方将语音内容审核示例接到 `video/check-sync.json`，长视频语音覆盖须另外落实及验收，缺少结果时保留覆盖缺口。 [签名](https://sightengine.com/docs/callback-signature) · [视频与语音端点](https://sightengine.com/docs/reference) |
| Hive | Hive Model API `https://api.thehive.ai/api/v2/task/sync`，各项目用自己的 token。文本使用 `text_data`，视觉用 `media` 或 URL。语音、OCR 与视觉模型按项目能力配置。 [认证](https://docs.thehive.ai/reference/authentication) · [媒体参数](https://docs.thehive.ai/reference/submitting-via-task-api) | V2 `/api/v2/task/async` 生成项目内任务 ID。公开查询参考仍列 V1 `/api/v1/task`、`task_id` 与长视频／音频分页。V2 ID 是否能经该读入口查询、项目权限和回调签名需要供应商文档或授权项目确认；目前不假定 V2 查询路径，也不将未验签回调作为最终结果。Hive Moderation Dashboard 路径归另一产品。 [V2](https://docs.thehive.ai/reference/api-reference-introduction) · [公开查询参考](https://docs.thehive.ai/reference/get-task-summary) |

## 公共接缝 {#contracts}

- `openrust-content-moderation` 作为可选包。应用通过 Provider、`ModerationManager::extend`、具名 profile 与 `Moderation` 门面装配，复用既有短信／翻译的每应用工厂和实例缓存。替换工厂或配置后显式清除实例，手中句柄继续使用原快照。任一 HTTP、Artisan 或 Worker 进程均独立注册工厂。
- 对外使用 `TextInput`、`ImageInput`、`VideoInput`、`TaskId` 和 `Assessment` 等类型。小的内嵌图片有固定入口上限，私有图片／视频以具名 Storage 磁盘、对象键和内容版本标识。授权下载链接在供应商传输阶段生成，任务中保留私有对象引用与期望版本。文本字符上限、图片 MIME／格式、帧和音频预算由各驱动按协议缩紧。
- `ModerationDriver` 的文本、图片、视频提交、查询与可选取消可独立实现，未提供的入口返回指定能力的错误。视频提交只返回供应商任务身份；持久层保存父审核、子检查、期限、凭据版本及领取信息。每次查询使用原 profile、项目和凭据修订，回调路径复用 `webhook-client`。未获独立验签的回调通过已有提示入口记录，经认证查单复核后才推进完成状态。
- `Assessment` 保留原始供应商标签、归一化结论和已完成／部分／缺失的模态覆盖。调用方必须给出必检模态集合；缺任何一项时无法得到有效 Allow。取样、截断、Unknown、失败和取消分别记录，风险证据仍可提示拦截。持久父任务以子检查的完整终态和内容修订决定发布资格，人工或旧修订裁决遵循原先计划。
- 阿里云适配复用框架 `AliyunRpcSigner`。腾讯 SMS 签名中固定了 `sms` 服务作用域，内容审核需要按 `tms`／`ims`／`vm` 的实际规范校验；共享签名应保留已有短信测试。四家均复用框架 HTTP Client，给响应、重定向、文件读取和 HTTP 诊断脱敏设置明确边界。测试以本地协议替身验证，真实业务准确率、授权和费用按 CM-04 分别记录。

## CM-01 可选包和外部消费者 {#external-drivers}

`content-moderation` 功能启用可选的 `openrust-content-moderation`。包 Provider 发布 `config/moderation.rs`，默认配置名来自 `MODERATION_DRIVER`。公开文本／媒体输入保持原文与私有对象版本，任务 ID 的反序列化复用构造校验。覆盖结果用 `Assessment::decision` 检查调用方明确要求的模态。未知或部分覆盖没有隐式的有效 Allow。查任务需要调用应用选定的原具名驱动，默认驱动不会隐式接管任务。

包的外部消费测试覆盖输入、任务 ID 序列化、Allow 的覆盖约束、工厂失败重试、应用隔离、具名实例复用和清除后的旧句柄。独立应用登记仅文本驱动及媒体驱动，使用默认和具名入口。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过隔离迁移、配置发布及重复发布、两次真实 HTTP、两次独立 Artisan 读取、两次 Artisan 提交与两次独立 Worker。配置和路由缓存生成后，再改进程环境的媒体配置标记，原任务仍可解析。原生队列仅保存任务 ID；Worker 核对缺语音完整覆盖无法给出 Allow。HTTP 流程模拟驱动结果，视频持久状态、媒体读取、网络协议及回调仍由后续批次完成。

编译后的正式 `main` 另通过自动发现、`config:show moderation`、`vendor:publish`、`config:cache` 和实际 `/up`。首轮夹具引用了不存在的迁移常量，改为实际迁移类型。随后删除过时的根应用语言文件复制步骤，使用框架已内置的迁移语言资源。

冻结提交 `b9a751e80` 上叠加本批文件，Rust 1.94.0。3 项包测试、全工作区 `fmt`、包所有目标及应用验收目标的严格 Clippy 通过。三语用法的 Provider 与调用片段拼接为独立消费者编译，三语文档站内容检查和构建通过。日志位于 `target/tmp/content-moderation`。本批验毕外部驱动底座，四家协议、持久视频恢复、内容安全效果和服务开通仍按后续门禁验收。

## CM-02A 阿里云通用文本 {#aliyun-text}

固定 Alibaba Cloud Go SDK `v3.2.4` 的 `TextModerationWithOptions` 使用 HTTPS POST、RPC formData 和 `Green/2022-03-02`。Rust 复用现有 `AliyunRpcSigner` 和 HTTP Client。环境引用随配置缓存保存，真实密钥和可选 STS token 由各进程读取。出站请求不跟随重定向，限制连接和响应，并对 HTTP 观测启用原文／响应脱敏。业务码与 HTTP 状态分别映射。 [固定 Go SDK](https://github.com/alibabacloud-go/green-20220302/blob/v3.2.4/client/client.go#L1233) · [通用文本协议](https://help.aliyun.com/zh/document_detail/434034.html)

请求逐字保留 UTF-8 文本，最多 600 Unicode 字符。各业务场景的语言能力由 profile 的 `text_languages` 显式声明；不匹配的提示在请求前拒绝。高风险映射为 Block，低／中风险及缺少明确等级的非空证据交 Review；只有明确空标签且没有风险提示的组合可给出文本 Allow。保留供应商标签与请求 ID，原始风险词不进入公开结果。 [供应商场景与语言](https://help.aliyun.com/zh/document_detail/464388.html)

基线 `967a83a5`，Rust 1.94.0。`tests/moderation_aliyun_acceptance.py` 的本机 HTTP 替身独立计算 20 个请求的 ACS3 签名、正文哈希、nonce、STS 和 form 字段。验证真实应用 HTTP、独立 Artisan、配置／路由缓存和凭据轮换。轮换后以新的进程凭据签名，缓存中只有环境名。覆盖 600／601 字符、语言配置、未知标签、无明确风险等级、缺字段、非法与超大响应、拒绝重定向、HTTP 与业务限流、超时及服务错误。补语言提示门禁后整批重测通过。外部驱动 SQLite 的 HTTP／Artisan／Worker 和 3 项公开契约一并复跑。全工作区格式、包所有目标与验收应用严格 Clippy、三语内容检查通过。

本机协议替身使用回环 HTTP，正式端点使用 HTTPS。真实阿里云站点、账号权限、实际检测效果及费用仍待授权环境。图片、视频、语音／OCR 和其他三家适配仍按 CM-02／CM-03 推进。

## CM-02A 阿里云私有图片 {#aliyun-image}

按 `Green/2022-03-02` 的 `ImageModeration` 发送 `ServiceParameters.imageUrl`。具名 profile 约束私有 S3 磁盘及单一 HTTPS 下载来源，复用 Storage 的 `temporary_url_with` 生成五分钟的签名 URL，检查签名和唯一版本参数。拒绝磁盘不符、没有不可变版本及来源不匹配。原始临时 URL 留在单次请求中，错误和 HTTP 观测使用脱敏路径。 [固定 SDK](https://github.com/alibabacloud-go/green-20220302/blob/v3.2.4/client/client.go#L647) · [图片协议](https://help.aliyun.com/zh/document_detail/2528742.html)

保留原始标签、0–100 置信分、请求 ID 和高风险处置。采样帧信息只计部分覆盖。`nonLabel` 有检测项全关闭的歧义，计为部分覆盖，从而阻止有效 Allow。当前结果只声明视觉覆盖。 [覆盖歧义依据](https://help.aliyun.com/zh/document_detail/2857796.html)

基线 `c774e1e2`，Rust 1.94.0。`tests/moderation_aliyun_image_acceptance.py` 使用固定 SeaweedFS 4.53 镜像和阿里云 HTTP 协议替身。在同一个私有 S3 对象写入两个不同 PNG 版本，匿名下载返回 403。真实 Artisan 和应用 HTTP 发送签名请求，替身独立复算 ACS3 并把已签名路径转发到实际 SeaweedFS，分别读取和核对原、最新版本的像素原文。另验无效置信分、欠缺结果、采样与 `nonLabel`、不存在的版本、配置／路由缓存、跨磁盘和来源拒绝。复跑阿里云文本及 SQLite 外部驱动 HTTP／Artisan／Worker。首次本机无固定镜像，拉取该摘要后完成验收。

全工作区格式、内容审核包和验收应用的严格 Clippy、三语文档内容检查与 Python 语法通过。协议替身将 HTTPS 下载入口的签名路径转发到隔离回环 S3，生产 HTTPS 和真实阿里云下载、图片效果、OCR 以及托管存储仍待各自验收。

## CM-02B 腾讯云标准文本 {#tencent-text}

固定产品版本 `2020-12-29`，在 `tms` 服务作用域调用 `TextModeration`。从短信已有 TC3 代码提取 `TencentRpcSigner`，保留 `content-type;host` 及短信的 `sms`、`SendSms`、`2021-01-11`。审核新增具名区域／策略和临时令牌，凭据由各进程从配置引用解析。 [TC3 签名](https://intl.cloud.tencent.com/document/product/1145/50458) · [标准文本动作](https://cloud.tencent.com.cn/document/api/1124/51860)

请求发送原文的 UTF-8 Base64，按 10,000 Unicode 字符限长，明确 `Type=TEXT`。中文提示映射 `zh`，英文提示要求具名配置显式启用；正式开通按腾讯云业务要求核对。结果与原 `BizType` 匹配，合并总体及明细建议时保留风险较高的裁决，保留原始标签／子标签和 0–100 原始分。未知标签不会给出 Allow，缺字段及未知建议按协议错误处理。原始关键词和厂商错误消息不进入返回和日志。

基线 `950f5bbb`，Rust 1.94.0。`tests/moderation_tencent_text_acceptance.py` 的独立 HTTP 替身复算 19 个请求的 TC3 签名，核对时间、作用域、Action、版本、Token、策略、源语言和原始字节。运行真实应用 HTTP 与独立 Artisan，覆盖配置／路由缓存、凭据轮换、语言及 10,000／10,001 字符门禁、风险明细、未知标签、跨策略结果、限流、超时、重定向、权限及脱敏。复跑阿里云文本及私有 S3 图片 HTTP／Artisan。真实腾讯云账号、英文授权、检测效果和费用仍待供应商验收。独立的短信供应商 TCP 协议回归、审核包 3 个契约测试、全工作区格式检查、框架短信／审核包／生成应用严格 Clippy 和三语文档检查通过。

## CM-02B 腾讯云标准图片 {#tencent-image}

固定 IMS `2020-12-29` 的 `ImageModeration`，使用共享 TC3 及独立 `ims` 作用域，显式发送 `Type=IMAGE`。具名 `image_biz_type` 可以和文本策略同用，也可以单独启用。内联内容使用 `FileContent`，拒绝不足 16 字节和 Base64 编码达到 10 MB 的数据。私有对象仅经具名 S3 磁盘和受限 HTTPS 来源生成版本绑定的短期 `FileUrl`。供应商另检查图像格式、尺寸及出站下载限制。 [图片 API](https://cloud.tencent.com/document/product/1125/53273) · [响应结构](https://cloud.tencent.com/document/api/1125/53274)

分类、物体、风险库及 OCR 明细可提高总体风险级别。保留原始标签／子标签和 0–100 分，舍弃 OCR 原文、识别内容、坐标和链接。同步响应无完整性证明，视觉标为部分覆盖。有 OCR 结果时也只标部分覆盖，因此 `Pass` 无法授权放行；仍可提交已发现的风险证据给应用。

基线 `355ea0854`，Rust 1.94.0。真实应用 HTTP／Artisan 在配置及路由缓存前后调用独立协议替身。替身复算 7 个 IMS 请求的签名，使用固定 SeaweedFS 4.53 下载同一私有 S3 对象的两个版本，核对版本、有效期、原始字节及匿名下载 403。覆盖内联、跨盘和来源拒绝、null 版本、不存在版本、部分覆盖、OCR 拦截、置信分范围、策略不符和敏感数据脱敏。腾讯文本 TCP 回归和阿里云私有图片 HTTP／Artisan 回归重新通过，三语文档内容检查通过。首次复跑阿里云夹具时项目磁盘守卫清除了构建目录，重新构建后先将应用二进制复制到本批验收目录再通过。 3 个包契约测试、全工作区格式及审核包和应用验收目标严格 Clippy 一并通过。真实腾讯云账号、图片及 OCR 策略开通、产品效果和费用继续按 CM-04 验收。

## CM-02C Sightengine 文本 {#sightengine-text}

使用 Sightengine API `/1.0/text/check.json` 的表单 POST，凭据以环境名引用，单次请求在正文发送 `api_user` 与 `api_secret`。具名配置选择规则或 ML，声明允许的语言和规则类别／模型。提供语言提示时按 ISO 639-1 主子标签核对。规则支持中文、英文等文档列出的语言；ML 支持各模型自身的语言范围，`self-harm` 只接受单项 `en`。 [API](https://sightengine.com/docs/reference) · [规则](https://sightengine.com/docs/text-moderation-rule-based) · [ML](https://sightengine.com/docs/text-moderation-ml-models)

规则保留命中类型和类别，ML 保留达到阈值的原始 0–1 分。出现命中时交由人工复审。缺少配置类别或 ML 类别、响应包含尚未识别的 ML 类别时，不能从清洁结果得出完整文本放行结论。词条原文、PII、凭据和供应商错误说明不进入公开结果或日志。

基线 `7767420c1`，Rust 1.94.0，`tests/moderation_sightengine_text_acceptance.py` 通过真实 HTTP、独立 Artisan、配置及路由缓存和凭据轮换验收。替身核对 19 个表单请求的编码、原始 UTF-8、能力字段、语言与凭据。覆盖 10,000／10,001 字符、未启用语言、缺类别、词条脱敏、ML 临界分、未知类别、模型分数无效、限流、超时、权限、3xx 和伪成功。复跑阿里云与腾讯云文本真实 HTTP／Artisan。 审核包 3 个契约测试、全工作区格式、审核包及应用验收目标的严格 Clippy 和三语文档内容检查通过。实际授权账号、扩展语言、模型效果和费用继续按 CM-04 验收。

## CM-02D Hive V2 文本 {#hive-text}

选用 Hive V2 Enterprise 文本项目的 `POST /api/v2/task/sync`，每个具名驱动绑定独立的项目数字 ID 与环境令牌。请求使用 `Authorization: Token`、表单 `text_data`，显式语言以 `options.input_language` 传入。按 UTF-16 码元限制 1,024，补充平面的字符占两个码元。应用核对内外项目 ID、任务 ID、`TEXT_CLASSIFICATION` 及 HTTP／业务状态。 [认证](https://docs.thehive.ai/reference/authentication) · [V2 请求](https://docs.thehive.ai/docs/classification-text) · [文本响应](https://docs.thehive.ai/reference/text-moderation)

必检类别在各段合并最高 0–3 严重度。分数为 -1、缺少类别或响应语言与提示不一致时保留部分覆盖；达到配置阈值仍保留复审证据。词表、PII 和自定义模式仅保留命中类别，去除原文、匹配值和供应商错误消息。

基线 `004db8a9d`，Rust 1.94.0。真实 HTTP 与独立 Artisan 对接验证 18 个回环协议请求。覆盖配置及路由缓存、令牌轮换、显式语言、UTF-16 边界、跨项目／任务／模型拒绝、多段取最高分、未知覆盖、无效或重复分项及拒绝重定向。 Sightengine 文本实际 HTTP／Artisan 回归、审核包 3 个契约测试、全工作区格式、审核包及应用验收目标严格 Clippy、三语文档内容检查通过。V2 Enterprise 项目和各模型实际许可、业务效果、样本保留与费用仍需供应商验收；V2 异步查单协议继续单独核对。

## CM-02C Sightengine 图片 {#sightengine-image}

使用 `/1.0/check.json` 的 POST 表单。内联走框架既有 multipart，版本化私有图片复用短时 HTTPS S3 下载链接。具名 profile 选择 `nudity-2.1`、`violence`、`gore-2.0` 和 `text-content-2.0`，OCR 类别显式配置。HTTP 请求共用响应上限、无重定向和脱敏设置。 [API](https://sightengine.com/docs/reference) · [视觉模型](https://sightengine.com/docs/models) · [OCR](https://sightengine.com/docs/ocr-text-moderation-in-images-2.0)

对返回的单图或采样帧提取风险标签及各标签最高 0–1 分。OCR 按类别给出证据，丢弃正文、邮箱和命中详情。响应不能证明原件的完整帧覆盖，已有视觉／OCR 结果标记为部分覆盖，未返回的模态保持缺失，已发现的风险仍可进入复审。

基线 `7ff196e9c`，Rust 1.94.0。真实 HTTP 和 Artisan 向替身发出 7 个请求，替身解析 multipart 内联 PNG／双帧 GIF，另经固定 SeaweedFS 4.53 读取同一私有对象的两个版本并核对字节。确认匿名 403、拒绝跨盘／null 版本／越权来源、无效分数、缺模型、缓存及敏感数据脱敏。替身构造选定帧及其 OCR 响应以复核最高分及部分覆盖。初次构建夹具的借用声明不完整，且 Artisan 位置参数与自动 `--version` 重名，均修正后重测。Sightengine／Hive 文本和腾讯云／阿里云图片回归、3 个包契约、全工作区格式、包和验收应用严格 Clippy、三语文档检查通过。真实 Sightengine 项目、支持的模型与 OCR 语言、动画采样质量和实际费用仍待 CM-04 验收。

## CM-02D Hive V2 视觉图片 {#hive-image}

具名 profile 的 V2 Enterprise 视觉项目单独绑定项目 ID、令牌环境名和响应模型类型。复用文本驱动的 V2 认证和响应身份校验。分类参考示例使用对象及数字状态，文本示例使用列表及字符串状态，两种已文档化的形态均只接受一项结果。内联图片用 multipart 的 `media`，存储图通过限定来源和固定版本的 HTTPS `url` 发送。 [视觉概览](https://docs.thehive.ai/docs/visual-content-moderation) · [媒体字段](https://docs.thehive.ai/reference/submitting-via-task-api) · [分类响应](https://docs.thehive.ai/reference/classification)

按配置的风险类别提取采样帧里最高 0–1 分，命中阈值时复审。缺少任何可用类别时视觉覆盖缺失，有采样证据时保持部分。OCR 的项目协议单独验收。省略临时 URL、源媒体 URI 和供应商错误说明。

基线 `e3f6ebc78`，Rust 1.94.0。真实 HTTP／Artisan 的 V2 回环替身有 10 次视觉请求与 1 次同 profile 文本请求。对内联 PNG、双帧 GIF、真实 SeaweedFS 中两个私有 S3 版本分别核对授权、请求数据、过期时间和读取字节。覆盖具名项目令牌隔离、配置及路由缓存、403 匿名下载、拒绝不符的对象来源、跨盘、缺版本、分数和响应身份异常。首次夹具的请求助手名称遮蔽 Python HTTP 模块，改名后复测。 Hive 文本与 Sightengine 图片的 HTTP／Artisan 回归、3 个包契约、全工作区格式、审核包及验收应用严格 Clippy、三语内容检查同时通过。真实 Hive Enterprise 视觉项目、模型版本、区域授权、效果与费用仍待 CM-04 验收。

## CM-02D Hive V2 OCR {#hive-ocr}

独立 `OCR_MODERATION` 项目复用 Hive V2 的认证、响应身份和媒体传输。具名项目分别绑定词类、0–3 严重度和指向凭据的名称。同一素材可对视觉和 OCR 两个具名项目分别调用 `image()`，每个结果报告自己的请求 ID 和模态。OCR 按识别块合并最高级别以及模式命中类型，省略原文、匹配值、词表和坐标。 [OCR 响应](https://docs.thehive.ai/reference/ocr-moderation)

明确 `PHOTO`、原件一帧、时长与输出时间均为零时，配合已返回的帧和已配置类别建立静态结果覆盖。OCR 无识别块的同一静态帧可报告完整。动图、缺少帧数、类别不可用或证据不全保持部分，完全缺少输出时记录无覆盖。Hive 视觉项目使用同样的静态元数据规则。

基线 `f74bbcb9a`，Rust 1.94.0。10 次本机协议请求由真实 HTTP／Artisan 触发，核对独立令牌、同一私有 S3 对象的两个版本、匿名 403、配置和路由缓存、双帧 GIF 采样、静态图完整与空识别、不可用类别和缺字段、模型错配、超分以及隐私截断。复跑 Hive 视觉／文本 HTTP 和 Artisan，视觉增验静态完整条件。3 个包契约、全工作区格式、审核包与应用验收目标严格 Clippy、三语文档检查通过。真实 OCR 项目许可、识别质量与地区费用继续按 CM-04 验收。

## CM-03A 阿里云点播提交与查单 {#aliyun-video}

对照 `Green/2022-03-02` 与固定 Go SDK `v2.23.0`，复用 ACS3 HTTPS RPC formData。提交使用 `VideoModeration` 的 `ServiceParameters.url`，查单用同一服务的 `VideoModerationResult` 与 `taskId`。中国内地 `videoDetection` 对应上海、北京和杭州，`videoDetectionByVL` 对应上海和北京。点播取消能力返回 Unsupported，直播取消是另一契约。 [提交源码](https://github.com/alibabacloud-go/green-20220302/blob/v2.23.0/client/client.go#L1265) · [查单源码](https://github.com/alibabacloud-go/green-20220302/blob/v2.23.0/client/client.go#L1389) · [点播 API](https://help.aliyun.com/zh/document_detail/2505810.html)

回传 288／280 区分排队及运行，200 完成结果必须匹配原 ID。视频下载和格式失败映射 Failed，409 表示结果不可再查并映射 Expired，403／480 单独报告限流。提交响应丢失、远端失败或成功响应缺少合法 ID 时报告 `SubmissionUnknown`，没有盲目重试。供应商建议首次查询至少等 30 秒，有效结果保留 24 小时。私有视频沿用来源和不可变对象版本校验。画面、语音、OCR 分开记录覆盖，允许在部分覆盖时保留复审或拦截证据。帧和语音时间按原素材时间轴转换为毫秒，省略识别原文、供应商解释和证据下载链接。

基线 `11a2220e0`，Rust 1.94.0。真实 HTTP、Artisan 及独立 SQLite 数据库 Worker 经替身复算 ACS3 签名。5 次提交和 15 次查询覆盖配置及路由缓存、服务／区域门禁、两个真实 MP4 私有对象版本、未接受／近线待定／运行／到期／失败、响应丢失、结果任务错配、非法片段及敏感数据过滤。测试使用 FFmpeg／FFprobe 生成并核对有声和无声样本，替身读取固定 SeaweedFS 4.53 上的对应版本。框架 Worker 的任务载荷仅含子任务 ID，独立查询证据与 HTTP 结果相同，退出后两张队列表为空。阿里云文本／图片 HTTP／Artisan 回归、3 个契约、全工作区格式、审核包和应用验收目标严格 Clippy、三语文档检查通过。

本批验证点播协议和跨进程查单。父子任务持久化、审核配置修订、回调验签与提前到达补偿、其他供应商以及实际阿里云账号的下载时限、采样质量、费用和保留设置继续跟踪。

## CM-03B 腾讯云标准点播提交、查单与取消 {#tencent-video}

标准 VM `2021-09-22` 复用 TC3，使用 `vm` 独立签名作用域。提交 `CreateVideoModerationTask` 带 `Type=VIDEO` 和具名 `BizType`，每次发送一个固定版本的媒体 URL。生成单次数据 UUID 并校验返回关联，缺失可确认的任务 ID 时报告 `SubmissionUnknown`。查单要求返回匹配 ID、点播类型和策略，分别处理待执行、进行中、失败、取消、完成。取消只发送给通过这些核对的等待中任务，供应商仍负责检查状态竞态。 [标准提交](https://cloud.tencent.com/document/api/1265/80017) · [查单](https://cloud.tencent.com/document/product/1265/80016) · [取消](https://cloud.tencent.com/document/api/1265/80018)

查单请求 `ShowAllSegments`。从回传结果提取画面、OCR、语音分数及原件时间轴，画面偏移按秒转换，音频分片时长和内部起止时间按毫秒叠加。总建议及明细只会提高风险，已收到的片段仍为部分覆盖。响应中的结果文件链接、原文及账号凭据均不进入结果或日志；远端片段明细文件与签名回调另行核对。 [数据结构](https://cloud.tencent.com/document/api/1265/80019)

基线 `e6f216055`，Rust 1.94.0。真实 HTTP、Artisan 与独立 SQLite 数据库 Worker 触发 5 次提交、21 次查单及 2 次取消，回环替身逐次独立验证 TC3。SeaweedFS 4.53 真实读取同键有声／无声两个 MP4 版本并核对 bytes。验证配置／路由缓存、关联 UUID、待处理取消／重复取消／运行竞态、错任务和错策略、覆盖、毫秒证据、限流与脱敏。队列表消费完成，Worker 与 HTTP 输出一致。腾讯文本、图片和阿里云点播的消费者回归、3 个包契约、全工作区格式、应用与审核包严格 Clippy、三语文档检查均通过。真实腾讯云策略、下载行为、结果明细、回调、质量及费用继续按 CM-03／CM-04 验收。

## CM-03C Sightengine 点播提交、查单与停止 {#sightengine-video}

使用 `/1.0/video/check.json` 的表单 POST 送审私有 S3 版本，返回 `med_` 媒体 ID。查单用 `GET /1.0/video/byid.json`，停止使用 DELETE 的表单体，均按公开协议发送账号凭据。复用 HTTP Client 的出站快照脱敏，事件不含实际查询串或正文。结果核对媒体身份与进度。视频与图片共用视觉、OCR 模型契约；帧位置转换为原件毫秒，不输出 OCR 原文。异步响应可能因时长限制提前完成，也可能只覆盖抽样帧，画面和 OCR 保持部分，语音保持缺失。官方语音审核另有同步入口。 [异步点播](https://sightengine.com/docs/moderate-stored-video-asynchronously) · [查单和停止](https://sightengine.com/docs/interact-with-video-moderation-jobs) · [语音](https://sightengine.com/docs/audio-moderation-model)

基线 `a8d0e2206`，Rust 1.94.0。本地协议替身通过真实 HTTP、Artisan、独立 SQLite Worker 收到 4 次提交、12 次查单、1 次停止。读取有声、无声两个真实 MP4 的私有版本，核对文件原文、模型和限时参数。覆盖了坏 ID、身份不符、未知进度／状态、限流、拒绝访问、跨版本及无完整覆盖时的门禁。真实 `RequestSending` 事件监听逐次验证 GET 查询串和 DELETE 请求体已脱敏。Sightengine 文本／图片、腾讯云点播消费者回归、3 个包契约、全工作区格式、包及验收应用严格 Clippy 与三语检查均通过。账号级回调密钥、真实视频语音路径、套餐和供应商效果继续按计划验收。

## CM-03D 供应商凭据的只读挂载与修订 {#credential-revisions}

四家内置工厂读取具名配置的 `credential_ref` 与 `credential_revision`，容器默认从受控绝对目录打开单份 JSON。加载时检查实际打开的文件、16 KiB 上限、链接和 Unix 权限。引用和修订限 ASCII 标识符，缺失或不合法立即失败。同一修订内的字段一次读取，支持令牌及同一 Hive 配置内的独立项目。应用可绑定 `ModerationCredentialProvider` 接入已有密钥服务。显式 `*_env` 配置保留，两种来源混用拒绝。生产目录和同一修订的内容由部署方保持只读和一致；撤销后应重启持有旧驱动的进程。

基线 `464432dcf`（当批其他提交无此包变更），Rust 1.94.0。Sightengine 使用挂载凭据完整执行两版本私有 MP4 的真实 HTTP、Artisan 和独立 SQLite Worker，共 4 次提交、13 次查单、1 次停止。配置缓存保留原修订，即使运行环境指定新修订或缺失的挂载路径，HTTP／Artisan／Worker 仍请求原供应商地址并使用原凭据。旧文件撤销、符号链接、FIFO、超长、过宽 Unix 权限及路径穿越在请求前失败。清理配置缓存并显式选择新修订后，独立替身核对新凭据。唯一哨兵扫描覆盖配置缓存、`config:show moderation`、HTTP 日志、公开结果、Artisan 输出、实际数据库队列载荷和出站观察事件。显式环境注入模式复跑完整点播协议，另复跑阿里云文本、腾讯云文本、Sightengine 文本和 Hive OCR。3 个包契约、修改范围格式及包／应用严格 Clippy 通过。三语站点构建通过，Chrome 实测两个挂载点的桌面／移动导航及全量章节。

这批记录属于 D-05 的供应商出站凭据。恢复时固定原审核配置及凭据修订、失败任务诊断、供应商回调签名密钥、Windows 挂载 ACL 和真实供应商轮换仍须独立验收。

## CM-03E 固定查单的配置修订 {#profile-revisions}

具名驱动的即时解析保持原入口。延后任务可用 `pin_profile` 固定独立修订表中的驱动、账号别名、策略、必检模态和凭据引用。定位符只包含受限元数据和规范化配置的 SHA-256 摘要。配置的实际值随部署保留；恢复以摘要对照原修订，每次重新解析工厂和凭据。配置被改写、被移除、定位符格式不受支持、缺凭据时在出站前失败。使用新账号、策略或凭据的任务应建立新修订。账号别名由部署确立，供应商协议有实际归属字段时仍按协议核对。

基线 `fd533ec1a`，Rust 1.94.0。SQLite 队列先持久写入旧修订定位符和原任务 ID，再重建配置缓存，将默认配置改为其他驱动，将同名即时配置改到新地址和凭据。独立 Worker、Artisan 和新 HTTP 进程均通过旧修订访问原替身，3 次查单结果一致，加上挂载回归共 16 次查单。恢复同名旧修订时分别变更账号、策略、地址、凭据引用，另验移除旧修订、缺失旧凭据和不支持的定位符版本，未增加供应商调用。数据库队列载荷只含定位符和任务标识，无供应商地址或秘钥哨兵。3 个包契约、修改范围格式及包／应用严格 Clippy 通过。显式环境模式与 SQLite 外部驱动消费者回归通过，三语站点构建与两挂载点 Chrome 验收通过。

本批补齐 D-01 的配置解析接缝。应用审核记录与对象版本的同库写入、租约与重新派发、多租户／跨库复验、人工凭据迁移和四家真实账号仍按 CM-03／CM-04 跟踪。

## CM-03F 父审核与子检查同库落单 {#review-storage}

包迁移注册及发布两个领域表。存储适配使用共享数据库时钟记录创建、截止及子检查可执行时间，短事务写入待处理的父审核及待提交的点播检查。父表在应用／租户范围内唯一约束幂等键，子表以父记录外键绑定。相同请求重试返回原审核和检查 ID，修改输入复用同一键报冲突。应用、租户按稳定标识隔离，物理排序规则不会合并大小写不同的标识。领域对象保留业务内容修订、原对象版本和固定配置定位符。

基线 `38836b1ec`，Rust 1.94.0。本机 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 分别执行实际 Artisan 迁移、空表回滚重建、迁移发布和重复发布。通过独立 SQL 测试守卫使子表插入失败，三种数据库都回滚父记录。配置和路由缓存后，各库由 8 个实际并发 HTTP 请求争同一幂等键，返回同一对记录，换对象版本返回 409。通过独立 Artisan 验证应用／租户隔离与原记录查找，独立 Worker 仅凭数据库队列中的审核 ID 和作用域读取原持久记录，内容修订、对象版本、配置修订与 HTTP 相同。存在审核时迁移回滚被拒。复跑固定配置修订和外部驱动消费者；3 个包契约、修改范围格式、包及应用严格 Clippy 通过。三语站点构建与双挂载点 Chrome 检查通过。

本批完成数据库意图与读回，状态推进、查询预算、远端接受后恢复、回调及父子多模态终态留在 CM-03 后续批次。

## CM-03G 点播领取、确定回执与提交未知 {#submission-lease}

应用或 Worker 通过 `VideoReviewService::submit` 推进已落单检查。数据库时钟和父审核版本控制短事务领取，同一父记录只产生一次远端提交尝试。出站使用原配置、凭据和对象版本，在事务外等待；成功记录远端任务号。明确的配置、输入、授权、限流和拒绝保留稳定原因。远端响应不确定或进程在领取后失联时，状态为 Unknown，后续调用可以在租约到期时进行补偿标记。截止时间关闭审核。过时版本的迟到响应不能写回当前状态。

基线 `6c25eed10`，Rust 1.94.0。真实 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各经 HTTP、Artisan 与独立 Worker 调用独立供应商替身，共各 6 次送审请求。替身从真实 SeaweedFS 读取固定键的两个 MP4 版本并比较文件原文。HTTP 持有出站请求时，另一 Artisan 看到正在提交并可建立其他审核，证明请求期间父记录事务已释放。另测重复调用、404 跨边界读取、实际终止提交进程、数据库控制租约到期、旧进程恢复后迟到回执、明确限流、远端无可确认 ID、提交前截止及已知任务截止后保留 ID、旧配置不存在。各库 Worker 的负载仅含范围和审核 ID。数据库队列表消费干净。SQLite 存储、外部驱动和固定修订回归、3 个契约、修改范围格式及包／应用严格 Clippy 通过。磁盘停线清理期间中断的文档全路由检查重建后通过，三语桌面及移动导航均实测。

本批以应用／Worker 公共入口推进送审。定时补派、查询租约和预算、任务 ID 关联回调及跨模态终态为独立验收项。

## CM-03H 持久查单预算和覆盖决定 {#query-budget}

`VideoReviewService::query` 按原配置修订读取已确认的供应商任务。数据库保存查单时间、累计次数和有期限的领取，正常待定按配置时间查询，临时错误按有上限的退避延后。领取时扣减预算，进程中止后继续使用累计计数。父审核版本保护截止后的状态和证据。结果保存归一化模态、分类、置信度和时间轴。全部必检覆盖时才提供审核决定，缺项保留已有证据和失败原因。

基线 `0e0173250`，Rust 1.94.0。SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 均跑通真实 HTTP、Artisan 和数据库 Worker。各库经替身发出 7 次确认查询，覆盖提前调用、并发请求共用查单租约、待定间隔、限流退避、预算耗尽、实际终止进程后继续累计、截止后迟到结果及 Unknown 不查。独立 Worker 按审核 ID 恢复证据，核对 Sightengine 帧时间和风险量程，库内不含 OCR 原文。外部驱动提供完整视觉时产出 Allow，视觉／语音／OCR 必检而只得到部分证据时保留失败。以控制数据库时间字段的办法复现到期，无须等待整个租约与退避窗口。现有挂载修订、SQLite 存储与外部驱动消费者回归，修改范围格式、包和应用严格 Clippy、3 个契约通过。三语文档构建和 Chrome 双挂载点桌面／移动验收通过。

本批预算针对单项审核。Scheduler 持久补派、供应商 `Retry-After`、账号／租户共享在途上限和回调联验继续按 D-02／D-06 推进。

## CM-03I 由 Scheduler 恢复持久审核 {#review-recovery}

原生 `moderation:recover` 按应用和租户限量扫描。共享数据库短租约保护派发，队列消息只含定位信息和父审核版本。对应 Worker 从原数据库键读取，按持久记录推进提交或查单。已入列消息丢失后，可在派发租约到期后补发。领域处理进度与收件独立，旧消息按版本消除。

基线 `2df0cb8d2`，Rust 1.94.0。本机 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各配 Database 队列，另以 SQLite／PostgreSQL 配 Redis 8 队列。真实 HTTP 建立意图，Artisan `schedule:run` 通过原生命令派发，独立 Worker 实际消费。五组均验证单轮上限、租户隔离、删除已入列消息、等待租约、两个独立命令竞争、重复投递、查询未到期不派发，以及到期补查后完整视觉给出 Allow。通过修改本地数据库时刻字段加速测试窗口。Redis 夹具经修正无认证配置及读取原生消息封套后复跑通过。SQLite 持久化、送审／查单和外部驱动消费者回归、3 个包契约、修改范围格式、包和应用严格 Clippy、三语站点构建及 Chrome 双挂载点验收通过。

本批使用本机共享后端和外部驱动测试实现。不同主机、托管队列、审核接收回调及账户共享使用量上限仍待独立验收。
