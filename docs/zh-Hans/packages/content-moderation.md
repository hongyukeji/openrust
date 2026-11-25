# 内容审核（扩展）

启用 `content-moderation` 功能，或在依赖 `openrust-content-moderation` 的应用中注册 `ModerationServiceProvider`。执行 `./artisan vendor:publish --tag=moderation-config` 发布配置模板。在应用的 Rust 配置中将 `moderation.drivers.comments.driver` 设为 `review-text`，并通过 `MODERATION_DRIVER=comments` 选择默认配置。

当前包提供自定义驱动契约、阿里云文本和已存储图片审核，腾讯云文本和图片审核，以及 Sightengine 和 Hive 的文本审核。其他供应商操作及持久化视频编排正在实施。

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

## Hive 文本 {#hive-text}

在具名配置设定 `driver: "hive"`、Hive V2 Enterprise 文本项目的数字 ID `text_project`，以及引用该项目令牌的环境变量名 `token_env`。用 `text_classes` 指定必检类别，`text_review_severity` 取 1、2 或 3。如需显式传入语言提示，在 `text_languages` 列出该项目开通的语言。未传提示时由 Hive 自动检测。

请求上限为 1,024 个 UTF-16 码元。适配器核对响应项目、模型类型和任务 ID。类别分数为 -1 或必检类别缺失时，文本只计为部分覆盖。多段响应取类别的最高严重级别。模式匹配证据保留来源类别，省略命中原词及个人数据。 [Hive V2 文本 API](https://docs.thehive.ai/docs/classification-text)

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

提交后按原具名驱动和配置修订查询 `TaskId`。可序列化的任务 ID 仅包含远端标识。持久化时由应用保留配置、凭据修订和对象版本。待处理、失败或必检覆盖不完整时均无法批准发布。

通过 `Assessment::decision` 检查所需模态。结果不完整时仍可保留人工复核或拦截所需的风险证据。若业务要求语音审核，部分语音覆盖的成功结果无法产生 `Allow`。
