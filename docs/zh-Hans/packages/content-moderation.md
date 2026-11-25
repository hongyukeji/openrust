# 内容审核（扩展）

启用 `content-moderation` 功能，或在依赖 `openrust-content-moderation` 的应用中注册 `ModerationServiceProvider`。执行 `./artisan vendor:publish --tag=moderation-config` 发布配置模板。在应用的 Rust 配置中将 `moderation.drivers.comments.driver` 设为 `review-text`，并通过 `MODERATION_DRIVER=comments` 选择默认配置。

当前包提供驱动契约和应用装配。内置云服务适配器及持久化视频编排正在实施。

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
