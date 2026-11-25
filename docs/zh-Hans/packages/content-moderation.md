# 内容审核（扩展）

启用 `content-moderation` 功能，或在依赖 `openrust-content-moderation` 的应用中注册 `ModerationServiceProvider`。执行 `./artisan vendor:publish --tag=moderation-config` 发布配置模板。在应用的 Rust 配置中将 `moderation.drivers.comments.driver` 设为 `review-text`，并通过 `MODERATION_DRIVER=comments` 选择默认配置。

当前包提供自定义驱动契约、阿里云文本和已存储图片审核，腾讯云文本和图片审核，Sightengine 文本和图片审核，以及 Hive 文本和视觉审核。其他供应商操作及持久化视频编排正在实施。

## 供应商凭据 {#credentials}

生产部署可将 `moderation.credential_directory`（默认读取 `MODERATION_CREDENTIAL_DIRECTORY`）设为受控只读挂载的绝对路径。在具名配置中填写 `credential_ref` 和 `credential_revision`。例如引用 `sightengine` 和修订 `r1` 会读取 `sightengine.r1.json`，其中的 JSON 对象填写 `api_user` 和 `api_secret`。配置缓存保存目录、引用和修订，HTTP、Artisan 和 Worker 都需要读取同一挂载。

下文的 `*_env` 键供显式环境变量注入使用。使用挂载文件时，移除这些键，并在文件中使用去掉 `_env` 后缀的字段名。每个文件最多 16 KiB。Unix 上可用 `0400` 或 `0440`，目录和文件由部署方控制。轮换时新增修订文件，保留仍被旧任务引用的版本，撤销时移除旧文件并重启持有旧驱动实例的进程。应用可在容器中绑定 `dyn ModerationCredentialProvider`，从已有密钥服务读取相同引用和修订。

## 阿里云文本 {#aliyun-text}

在 `moderation.drivers.aliyun_comments` 设置 `driver: "aliyun"`、`region: "cn-shanghai"`、`text_service: "comment_detection"`、`access_key_id_env` 和 `access_key_secret_env`。最后两个字段填写环境变量名。使用 STS 时还需设置 `security_token_env`。用 `Moderation::driver("aliyun_comments")` 选择配置后调用 `text`，也可用 `MODERATION_DRIVER=aliyun_comments` 设为默认。

适配器调用 `Green/2022-03-02` 的 `TextModeration`，最多接受 600 个 Unicode 字符。高风险返回 `Block`，低风险和中风险返回 `Review`。明确的空标签且无风险时，文本覆盖可得到 `Allow`。未识别的标签保留在结果中。 [阿里云文本审核增强版](https://help.aliyun.com/zh/document_detail/434034.html)

如果传入语言提示，在 `text_languages` 列出所选服务支持的语言标签。不在配置内的提示在发请求前拒绝。未提供提示时，原文直接交由所选服务审核。

## 阿里云图片 {#aliyun-image}

审核已存储图片时，配置 `image_service`（例如 `baselineCheck`），以 `media_disk` 指定私有 S3 磁盘，并将 `media_origin` 设为该磁盘的 HTTPS 下载来源。向具名驱动传入 `ImageInput::stored(StoredMedia::new(disk, key, version_id)?)`，其中 `version_id` 是不可变的 S3 对象版本。驱动向阿里云发送绑定该版本、有效期五分钟的签名地址。来源或磁盘不匹配、对象未版本化时，在提交前拒绝。

此操作提供视觉覆盖。截帧采样或语义不明确的 `nonLabel` 只计部分覆盖。若响应含有置信分，通过 `Assessment::confidence` 读取原值与 0–100 量程。业务还要求 OCR 时应检查覆盖字段。 [阿里云 ImageModeration](https://help.aliyun.com/zh/document_detail/2528742.html)

## 腾讯云文本 {#tencent-text}

在具名配置设置 `driver: "tencent"`、腾讯云开通的 `region` 和 `biz_type`，并填写环境变量名 `secret_id_env`、`secret_key_env`。临时凭据另设 `security_token_env`。适配器使用 UTF-8 Base64 调用标准 `TextModeration`，最多接受 10,000 个 Unicode 字符。

中文提示映射为 `zh`。腾讯云确认账号支持英文选项后设置 `english_text: true`，纯英文文本的提示才会转换为 `en`。适配器核对响应策略与配置的 `biz_type`。明细可提高总建议的风险级别，未识别标签无法得到 `Allow`。 [腾讯云 TextModeration](https://cloud.tencent.com.cn/document/api/1124/51860)

## 腾讯云图片 {#tencent-image}

用 `image_biz_type` 指定已开通的标准图片策略。具名配置可单独启用图片，也可同文本共用。标准 `ImageModeration` 接收内联图片的 Base64（原始内容至少 16 字节，编码后小于 10 MB），也可用五分钟有效、固定私有 S3 对象版本的签名链接。存储图片需要配置 `media_disk` 和已授权的 HTTPS `media_origin`。

适配器保留画面和 OCR 的风险结果与原始分数。腾讯云可能抽帧或切分长图，同步响应无法证明完整覆盖，因此厂商返回 `Pass` 时仍无法得出放行结论。识别正文、命中词和临时链接不进入对外结果。 [腾讯云 ImageModeration](https://cloud.tencent.com/document/product/1125/53273)

## Sightengine 文本 {#sightengine-text}

在具名配置设置 `driver: "sightengine"`、`api_user_env` 和 `api_secret_env`，在 `text_languages` 列出启用的语言代码。`text_mode: "rules"` 使用 `text_categories`，例如 `["profanity", "personal"]`。`text_mode: "ml"` 使用 `text_models` 和 0 至 1 的 `text_review_score`。`self-harm` 模型要求 `text_languages: ["en"]`。

适配器保留规则或模型标签及 ML 命中的原始分数。分数达到配置阈值时进入复审，缺少已配置类别的结果时覆盖为部分。最多接收 10,000 个 Unicode 字符，命中的原文和 API 凭据不进入结果或日志。 [规则](https://sightengine.com/docs/text-moderation-rule-based) · [ML 模型](https://sightengine.com/docs/text-moderation-ml-models)

## Sightengine 图片 {#sightengine-image}

在 `image_models` 选择具名配置所用模型，并将 `image_review_score` 设为 0 至 1 的视觉复审阈值。支持 `nudity-2.1`、`violence`、`gore-2.0`、`text-content-2.0`。启用 OCR 时用 `image_text_categories` 指定检查类别，例如 `["email", "insult"]`。

内联图片经 multipart 提交。存储图片用 `media_disk` 和 `media_origin` 生成固定私有 S3 对象版本的短时 HTTPS 链接。适配器保留返回帧中每个视觉标签的最高分和 OCR 类别，省略命中原文。覆盖完整性无法确认时标记为部分，结果无法放行。 [Sightengine 图片 API](https://sightengine.com/docs/reference) · [OCR](https://sightengine.com/docs/ocr-text-moderation-in-images-2.0)

## Hive 文本 {#hive-text}

在具名配置设定 `driver: "hive"`、Hive V2 Enterprise 文本项目的数字 ID `text_project`，以及引用该项目令牌的环境变量名 `token_env`。用 `text_classes` 指定必检类别，`text_review_severity` 取 1、2 或 3。如需显式传入语言提示，在 `text_languages` 列出该项目开通的语言。未传提示时由 Hive 自动检测。

请求上限为 1,024 个 UTF-16 码元。适配器核对响应项目、模型类型和任务 ID。类别分数为 -1 或必检类别缺失时，文本只计为部分覆盖。多段响应取类别的最高严重级别。模式匹配证据保留来源类别，省略命中原词及个人数据。 [Hive V2 文本 API](https://docs.thehive.ai/docs/classification-text)

## Hive 图片 {#hive-image}

将 `image_project` 设为 V2 Enterprise 视觉项目的数字 ID，`image_token_env` 设为保存该项目令牌的环境变量名。`image_model_type` 使用项目报告的模型类型，`image_classes` 列出需要检查的风险类别，`image_review_score` 是 0 至 1 的复审阈值。同一具名配置可启用文本和视觉，各自使用独立的项目 ID 与令牌。

内联图片经 multipart 发送。存储图片根据 `media_disk`、`media_origin` 生成固定私有 S3 对象版本的短时 HTTPS 链接。适配器保留采样结果中每个已配置类别的最高分。供应商未证明整个图像序列已处理时，视觉覆盖保持部分。OCR 可通过单独的 Hive 项目配置。 [Hive 视觉审核](https://docs.thehive.ai/docs/visual-content-moderation)

## Hive OCR {#hive-ocr}

在具名 `hive` 配置填写 `ocr_project`、`ocr_token_env`、`ocr_classes` 和 `ocr_review_severity`。OCR 项目使用 Hive 的 `OCR_MODERATION` 模型。对该配置调用 `image()`，判断结果时指定 `Modality::Ocr`。如果业务同时要求视觉和 OCR，调用两个具名项目并检查各自的结果。

响应确认静态图仅一帧、审核时间戳为零，且每个识别块都有所有必检类别结果时，可计为完整覆盖。动图和证据不全时保持部分覆盖。适配器保留类别级证据和原始严重度，省略 OCR 原文、命中值和坐标。 [Hive OCR 审核](https://docs.thehive.ai/reference/ocr-moderation)

## Hive 点播提交 {#hive-video}

在具名 `hive` 配置中，将 `video_project` 设为 Hive V2 Enterprise 视频项目的数字 ID。将 `video_callback_url` 设为应用的 HTTPS 回调路由，并用 `media_disk` 和 `media_origin` 指定私有对象来源。显式使用环境变量时配置 `video_token_env`，使用凭据文件时填写 `video_token`。

`submit_video()` 将指定版本的对象交给 V2 异步接口。Hive 应答通过项目和实际回显的回调地址核对后，返回供应商任务 ID。回执不明确时先核对原任务，再决定是否重提。将返回 ID 用于跟踪任务，发布前应通过项目接入的通道确认审核结果。 [Hive 创建任务](https://docs.thehive.ai/reference/submitting-via-task-api)

如果 Hive 已为该项目开通 V1 Task Summary 接口，可设置 `video_query_api: "v1-task-summary"`，在 `video_classes` 中列出类别，并将 `video_review_score` 设为 0 至 1。该配置的任务可调用 `query_video()`。每次最多读取四页，每页 64 帧，并保留类别的最高分和对应时间。画面采样或分页受限时，覆盖保持部分。 [Hive 查单接口](https://docs.thehive.ai/reference/get-task-summary)

各项视频检查使用独立项目和凭据。画面文字将 `video_model_type` 设为 `OCR_MODERATION`，语音设为 `TRANSCRIPTION`，两者都用 `video_review_severity` 设置 1、2 或 3 级复核门槛。OCR 证据保留对应帧时间，语音证据保留分类严重度。归一化结果仅提供类别证据，原始转写和匹配文字留在供应商响应。 [Hive OCR](https://docs.thehive.ai/reference/ocr-moderation) · [Hive 语音审核](https://docs.thehive.ai/reference/speech-moderation)

Hive 回调可以通过 Webhook Client 接收查单提示。将 `HiveLookupHint` 绑定到固定的 Hive 配置修订，在路由设置 `moderation-hive-hint` 验签器，将该实例分别注册为具名 `unverified_hint_receiver` 的策略及 `QueuedWebhookHint` 的处理器。提示队列和收件库使用同一数据库，多实例部署使用共享缓存执行入口限流。

仅在 Hive 项目已开通 `video_query_api = "v1-task-summary"` 查单时启用此入口。已登记任务必须与固定项目和配置修订相符，最多提前一次计划查单。回调原文不写入收件库。Worker 用原项目凭据向 Hive 查询并应用原有覆盖规则。 [Hive 任务回调](https://docs.thehive.ai/reference/submit-a-task-asynchronously)

## 阿里云点播视频 {#aliyun-video}

在具名 `aliyun` 配置设置 `video_service`。上海、北京和杭州支持 `videoDetection`，上海和北京支持 `videoDetectionByVL`。私有版本化 S3 对象使用 `media_disk`、`media_origin`，驱动为指定版本生成五分钟有效的 HTTPS 签名下载链接。

`submit_video()` 返回供应商任务 ID。保留原具名配置和 ID，再通过 `query_video()` 查单。阿里云建议提交至少 30 秒后首次查询，结果保留 24 小时。提交响应丢失时先核对 `SubmissionUnknown`，再决定是否新建任务。阿里云的远端取消接口适用于直播流。

完成任务仍可能只有部分画面、语音或 OCR 覆盖。`Assessment::evidence_span()` 使用原视频起点后的毫秒表示抽帧或语音证据。识别原文和证据临时链接均不进入结果。 [阿里云点播视频 API](https://help.aliyun.com/zh/document_detail/2505810.html)

接收带签名的视频通知时，将 `video_callback_url` 设为固定 HTTPS 入口，并在凭据挂载提供 `video_callback_seed`。使用环境变量凭据时，通过 `video_callback_seed_env` 指定变量名。Seed 使用 16–64 个 ASCII 字母、数字或下划线。在 Webhook Client 对应路由配置 `moderation-aliyun`、`moderation_callback_account_uid` 中的阿里云主账户 UID，以及 `moderation_callback_credential` 中的凭据修订。将 `AliyunCallbackProcessor` 绑定到同一账户别名和独立收件范围。

驱动选择 SHA-256 回调校验。验签器比较解码后原始 `content` 字符串和 `checksum`。终态通知可提前触发一次带认证的原任务查单，提前到达时可延期关联。表单正文遵循收件保留策略。 [阿里云视频回调](https://www.alibabacloud.com/help/en/content-moderation/latest/video-moderation-enhanced-api)

## 腾讯云点播视频 {#tencent-video}

为具名 `tencent` 配置设置标准点播策略 `video_biz_type`，并设定私有版本化 S3 素材的 `media_disk` 与 `media_origin`。驱动调用 VM `2021-09-22` 接口并使用 `Type=VIDEO`。

通过 `submit_video()` 提交，再用同一具名配置的返回 ID 查询。`cancel_video()` 用于等待中的任务。发送取消前核对任务类型和策略，期间已开始的任务可能被腾讯云拒绝取消。提交返回 `SubmissionUnknown` 时，先核对旧任务再考虑重新提交。

驱动将响应中的画面、语音和 OCR 证据按原件时间轴转换为毫秒。没有完整覆盖证明时，仍按部分覆盖处理。 [提交](https://cloud.tencent.com/document/api/1265/80017) · [查单](https://cloud.tencent.com/document/product/1265/80016) · [取消](https://cloud.tencent.com/document/api/1265/80018)

具名点播配置可用 `video_callback_url` 向腾讯云传入固定 HTTPS 通知入口。在账户凭据挂载中提供 `video_callback_seed`，使用环境变量凭据时用 `video_callback_seed_env` 指定变量名。Seed 长度为 16–128 字节，使用不含空格的可打印 ASCII。在 Webhook Client 的对应路由配置 `moderation-tencent` 验签器、凭据挂载引用 `moderation_callback_credential` 和 `TencentCallbackProcessor`。限制收件库访问并配置保留期限。

验签器按腾讯云校验和规则验证原始正文的 `X-Signature`。处理器核对路由、账户、策略和任务类型。若终态通知先于原任务 ID 写入，则延期处理；关联后提前执行一次带认证的查单。审核决定采用查单结果，并继续要求完整覆盖。 [腾讯云回调配置](https://cloud.tencent.com/document/product/1265/104001)

## Sightengine 点播视频 {#sightengine-video}

在具名 `sightengine` 配置填写 `video_models` 和 `video_review_score`，启用 OCR 时另填 `video_text_categories`。`video_max_duration` 默认 1,800 秒，套餐允许时可设至 3,600 秒。用 `media_disk` 和 `media_origin` 指定版本化私有素材来源。

`submit_video()` 返回媒体 ID，用原具名配置查询或停止任务。完成状态可能来自时长限制或对部分帧的采样。异步入口只报告已取得的画面和 OCR 证据，语音覆盖保持缺失。HTTP 客户端的诊断事件从查询串及停止请求正文中省略凭据。 [异步视频](https://sightengine.com/docs/moderate-stored-video-asynchronously) · [任务操作](https://sightengine.com/docs/interact-with-video-moderation-jobs)

### 账户级视频回调 {#sightengine-callbacks}

同时启用 `content-moderation` 和 `webhook-client`。在 Sightengine 账户中将回调 URL 指向通过 Webhook Client 注册的路由。各供应商账户使用自己的路由配置，将 `signature_validator` 设为 `moderation-sightengine`。`moderation_callback_credential` 填写 `{"name":"sightengine-callback","revision":"r2"}` 这样的对象，将 `signing_secret` 写入挂载的 `sightengine-callback.r2.json`。选择应用的 Webhook Client 存储与处理 Job。

轮换期间，`moderation_previous_callback_credentials` 可保留至多三个早期引用。撤销时从路由配置中移除废弃引用。验签使用原始请求体，并校验五分钟时间窗。Sightengine 的逐请求回调 URL 发送无签名通知，签名路由应使用账户级 URL。 [Sightengine 回调签名](https://sightengine.com/docs/callback-signature)

持久审核可通过 `QueuedWebhookJob::register_with_database_deferred_processing` 注册 `SightengineCallbackProcessor`。构造时传入审核存储、固定配置使用的账户别名，以及 Webhook Client 的作用域、配置名称和修订。提前到达的回调由 Webhook Client 延期处理，等待可关联的供应商任务 ID。如果应用已知接收记录 ID，可调用 `QueuedWebhookJob::wake_deferred` 提前唤醒。

匹配的终态通知会关联到原审核，并使原供应商任务到期查单。`moderation:recover` 派发带认证的查单，审核继续使用原有查询预算、截止时间和必检覆盖要求。

## 自定义驱动 {#custom-drivers}

以下示例将文本送人工复核。在包 Provider 之后注册示例 Provider，HTTP、Artisan 和 Worker 都可解析相同的具名配置。

```rust
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use openrust_content_moderation::{
    Assessment, Coverage, Decision, Modality, ModerationDriver, ModerationFuture,
    ModerationManager, Operation, TextInput,
};
use std::{collections::BTreeMap, sync::Arc};

struct ReviewText;

impl ModerationDriver for ReviewText {
    fn supports(&self, operation: Operation) -> bool {
        operation == Operation::Text
    }

    fn text<'a>(&'a self, _: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async {
            Ok(Assessment::new(
                Decision::Review,
                BTreeMap::from([(Modality::Text, Coverage::Complete)]),
                vec![],
            ))
        })
    }
}

pub struct ReviewTextProvider;

impl ServiceProvider for ReviewTextProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<ModerationManager>()?
                .extend("review-text", |_| Ok(Arc::new(ReviewText)));
            Ok(())
        })
    }
}
```

```rust
use openrust_content_moderation::{Modality, Moderation, TextInput};

let input = TextInput::new("Sample text")?.with_language("en")?;
let result = Moderation::text(&input).await?;
let recommendation = result.decision(&[Modality::Text]);
```

使用 `Moderation::driver("comments")` 选择具名配置。`ModerationManager::forget_drivers` 使后续解析使用替换后的工厂和配置，已取得的句柄继续使用原快照。配置缓存保存配置值，各进程自行注册工厂。

## 媒体与异步结果 {#media}

`ImageInput::inline` 接受最多 10 MiB 的图片字节。驱动须核对实际格式，并可进一步缩小上限。私有对象使用 `StoredMedia`，视频使用 `VideoInput`。驱动应通过配置的存储核对对象版本。

需要延后查单时，在 `moderation.profile_revisions.<名称>.<修订>` 下声明完整配置，加入 `account`、`policy_revision`、`credential_ref`、`credential_revision` 和非空的 `required_modalities`。可选模态为 `text`、`visual`、`speech` 和 `ocr`。`account` 填部署时与该凭据配对的账号或项目别名。地址、模型、策略或凭据修订发生变化时创建新配置修订，并保留未完成任务引用的旧配置。

提交前用 `Moderation::pin_profile(name, revision)` 固定配置，用 `Moderation::driver_for_revision(&profile)` 解析驱动。应用将返回的配置定位符、远端 `TaskId` 和原对象版本一起持久化，查单时按该定位符重新解析。原配置被原地修改、移除或对应凭据无法读取时，恢复失败。待处理、失败或必检覆盖不完整时均无法批准发布。

通过 `Assessment::decision` 检查所需模态。结果不完整时仍可保留人工复核或拦截所需的风险证据。若业务要求语音审核，部分语音覆盖的成功结果无法产生 `Allow`。

## 持久化视频送审意图 {#video-review-storage}

启用扩展后执行 `./artisan migrate`。需要将两份迁移源码发布到应用时，执行 `./artisan vendor:publish --tag=moderation-migrations`。

使用数据库连接和稳定的应用、租户标识构造 `ModerationStore`，HTTP 和 Worker 使用相同标识。用幂等键、业务内容 ID 与修订、原始 `StoredMedia`、已固定的配置和超时秒数构造 `VideoReviewRequest`。调用 `stage_video` 在同一事务写入待审核记录和点播子检查。重试相同请求可拿到原记录。同一范围内用相同的键提交不同内容会产生冲突。通过 `find` 按审核 ID 从同一范围读取。

落单后用存储适配及当前应用的 `ModerationManager` 构造 `VideoReviewService`，通过 Worker 或应用调用 `submit(review_id)`。服务在数据库领取子检查，解析原凭据修订，并提交原对象版本。并发调用可能读到 `Submitting`。供应商返回可确认的响应时保存其任务 ID。响应含糊或领取后 Worker 中断时，后续调用在租约到期后可把状态记为 `Unknown`。先向供应商核对原任务，再决定是否创建新审核。达到截止时间的审核保持过期。

调用 `VideoReviewService::query(review_id)` 推进已确认的任务。可以在固定配置中设置 `query_interval_seconds` 和 `query_max_attempts`，默认 30 秒、120 次，两者各可设 1–300。服务在数据库中记录下次查单时间、租约和尝试次数，重启后仍使用同一预算。远端结果正常待定时使用配置间隔，暂时故障采用有上限的退避。HTTP 429 或 503 携带有效的 `Retry-After` 时，可以延后下次查询，支持等待秒数和 HTTP 日期。审核截止时间和尝试次数仍有效。等待期间可记录已验签回调或未验签查单提示，下一次查单仍须等到供应商指定的时间。

通过 `VideoReview::assessment()` 读取归一化证据，通过 `VideoReview::decision()` 读取审核决定。全部必检模态覆盖完整后，审核才有决定。供应商完成但覆盖不足时，审核保留证据并报告覆盖失败。

### 多项必检的审核 {#video-review-groups}

通过 `VideoReviewGroupRequest::new` 和 `ModerationStore::stage_video_group` 为同一内容修订和媒体版本保存 2–8 个不同的固定配置修订，显式选择各必检模态对应的配置。同一意图及相同顺序的检查返回原审核组。同一意图下更改顺序、配置、内容或媒体版本时返回冲突。

每个子检查独立提交和查单，可由原有 Worker 与恢复命令推进。用 `find_video_group` 读取汇总结果。每个必检项成功且覆盖完整后才生成决定，并采用最严格的建议，优先级依次为 Block、Review、Allow。应用仍应根据当前内容修订执行发布授权。

### 人工裁决与发布 {#video-review-decision}

应用鉴别操作者身份并完成授权后，使用内容 ID 和修订、预期决定修订、裁决、操作者 ID 与理由构造 `ReviewDecisionRequest`。按业务审核策略选择 `VideoReviewTarget::Single` 或 `VideoReviewTarget::Group` 调用 `VideoReviewService::record_decision`。自动结论的修订为 0。每次成功的人工裁决追加新修订并保留原始评估；使用过期修订的并发写入会被拒绝。全部必检项成功且覆盖完整后才可记录人工裁决。

用同一审核对象和当前内容 ID、修订调用 `effective_decision`。仅在有效决定为 `Allow` 且决定修订与应用接受的修订一致时继续发布。多项必检策略始终使用审核组。应用在自身发布流程内核对内容仍存在、仍可发布且修订一致，并在提交发布前重新读取有效决定。操作者 ID 和理由仅在授权的审计入口展示。

### 取消已保存的点播检查 {#video-review-cancellation}

调用 `VideoReviewService::cancel(id)`，冻结待处理或进行中的检查。取消使用数据库时钟和记录版本，已越过截止时间的检查标记为过期。晚到的 Worker 无法改写取消结果。

已有确认的任务 ID 且厂商支持停止时，服务用原配置尝试远端取消。分别通过 `provider_cancellation()` 查看供应商确认、拒绝、不支持或结果未确认等状态。重复请求返回已保存的状态。任务号未知或停止结果未确认时，应先与供应商核对再继续处理。

调用 `VideoReviewService::cancel_group(id)`，在同一事务冻结待处理或进行中组内的全部未结束检查。已结束检查保留原有证据。本地事务提交后，服务再尝试停止已确认的远端任务，并在对应检查记录供应商结果。

## 定时恢复视频审核 {#review-recovery}

对每个需要托管的应用和租户，将 `moderation:recover` 加入每分钟调度。调度进程和 Worker 共用数据库与队列。命令领取不超过 `--limit` 条到期审核，默认 100，范围 1–500，只将本地审核 ID 和版本等定位信息入列。

```sh
./artisan moderation:recover app tenant --connection=database --queue=moderation
./artisan queue:work database --queue=moderation
```

通过 `--database` 选择具名审核数据库。将 Worker 队列的保留或可见性超时设为长于恢复 Job 的 125 秒超时。数据库保存下次恢复的时间，消息丢失后可在 60 秒派发租约到期时再次入列。收到旧审核版本的消息时保持当前状态。
