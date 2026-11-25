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

## 阿里云点播视频 {#aliyun-video}

在具名 `aliyun` 配置设置 `video_service`。上海、北京和杭州支持 `videoDetection`，上海和北京支持 `videoDetectionByVL`。私有版本化 S3 对象使用 `media_disk`、`media_origin`，驱动为指定版本生成五分钟有效的 HTTPS 签名下载链接。

`submit_video()` 返回供应商任务 ID。保留原具名配置和 ID，再通过 `query_video()` 查单。阿里云建议提交至少 30 秒后首次查询，结果保留 24 小时。提交响应丢失时先核对 `SubmissionUnknown`，再决定是否新建任务。阿里云的远端取消接口适用于直播流。

完成任务仍可能只有部分画面、语音或 OCR 覆盖。`Assessment::evidence_span()` 使用原视频起点后的毫秒表示抽帧或语音证据。识别原文和证据临时链接均不进入结果。 [阿里云点播视频 API](https://help.aliyun.com/zh/document_detail/2505810.html)

## 腾讯云点播视频 {#tencent-video}

为具名 `tencent` 配置设置标准点播策略 `video_biz_type`，并设定私有版本化 S3 素材的 `media_disk` 与 `media_origin`。驱动调用 VM `2021-09-22` 接口并使用 `Type=VIDEO`。

通过 `submit_video()` 提交，再用同一具名配置的返回 ID 查询。`cancel_video()` 用于等待中的任务。发送取消前核对任务类型和策略，期间已开始的任务可能被腾讯云拒绝取消。提交返回 `SubmissionUnknown` 时，先核对旧任务再考虑重新提交。

驱动将响应中的画面、语音和 OCR 证据按原件时间轴转换为毫秒。没有完整覆盖证明时，仍按部分覆盖处理。 [提交](https://cloud.tencent.com/document/api/1265/80017) · [查单](https://cloud.tencent.com/document/product/1265/80016) · [取消](https://cloud.tencent.com/document/api/1265/80018)

## Sightengine 点播视频 {#sightengine-video}

在具名 `sightengine` 配置填写 `video_models` 和 `video_review_score`，启用 OCR 时另填 `video_text_categories`。`video_max_duration` 默认 1,800 秒，套餐允许时可设至 3,600 秒。用 `media_disk` 和 `media_origin` 指定版本化私有素材来源。

`submit_video()` 返回媒体 ID，用原具名配置查询或停止任务。完成状态可能来自时长限制或对部分帧的采样。异步入口只报告已取得的画面和 OCR 证据，语音覆盖保持缺失。HTTP 客户端的诊断事件从查询串及停止请求正文中省略凭据。 [异步视频](https://sightengine.com/docs/moderate-stored-video-asynchronously) · [任务操作](https://sightengine.com/docs/interact-with-video-moderation-jobs)

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

调用 `VideoReviewService::query(review_id)` 推进已确认的任务。可以在固定配置中设置 `query_interval_seconds` 和 `query_max_attempts`，默认 30 秒、120 次，两者各可设 1–300。服务在数据库中记录下次查单时间、租约和尝试次数，重启后仍使用同一预算。远端结果正常待定时使用配置间隔，暂时故障采用有上限的退避。

通过 `VideoReview::assessment()` 读取归一化证据，通过 `VideoReview::decision()` 读取审核决定。全部必检模态覆盖完整后，审核才有决定。供应商完成但覆盖不足时，审核保留证据并报告覆盖失败。
