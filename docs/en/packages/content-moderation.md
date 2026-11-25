# Content Moderation (Extension)

Enable the `content-moderation` feature or register `ModerationServiceProvider` in an application that depends on `openrust-content-moderation`. Run `./artisan vendor:publish --tag=moderation-config` to publish the configuration template. In your application's Rust configuration, set `moderation.drivers.comments.driver` to `review-text` and set `MODERATION_DRIVER=comments` for the default profile.

The package currently provides the driver contracts and application integration. The built-in cloud adapters and durable video orchestration are being implemented.

## Custom drivers {#custom-drivers}

This example sends every text assessment to manual review. Register its provider after the package provider so HTTP, Artisan, and Workers can resolve the same named profiles.

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

Call `Moderation::driver("comments")` to select an explicit profile. `ModerationManager::forget_drivers` applies replacements to future resolutions, while existing driver handles keep their original profile. Configuration caching stores profile data and each process registers its factories.

## Media and asynchronous results {#media}

`ImageInput::inline` accepts up to 10 MiB. The driver checks the actual format and may impose a lower limit. Use `StoredMedia` for private objects and `VideoInput` for video. Drivers must verify the object's version using the configured storage.

Use the original named driver and configuration revision to query a `TaskId` after submission. A serializable task ID contains only the remote identifier. Applications retain the profile, credential revision and object version when persisting work. A pending task, execution failure, or incomplete required coverage cannot authorize publication.

Check the requested modalities with `Assessment::decision`. Review and block can retain risk evidence when results are incomplete. A successful result with partial speech coverage cannot produce `Allow` for a policy that requires speech.
