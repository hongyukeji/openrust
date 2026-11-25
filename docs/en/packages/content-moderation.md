# Content Moderation (Extension)

Enable the `content-moderation` feature or register `ModerationServiceProvider` in an application that depends on `openrust-content-moderation`. Run `./artisan vendor:publish --tag=moderation-config` to publish the configuration template. In your application's Rust configuration, set `moderation.drivers.comments.driver` to `review-text` and set `MODERATION_DRIVER=comments` for the default profile.

The package provides custom driver contracts, Alibaba Cloud text and stored-image moderation, Tencent Cloud text and image moderation, Sightengine text and image moderation, and Hive text and visual moderation. The other provider operations and durable video orchestration are being implemented.

## Provider credentials {#credentials}

For production, set `moderation.credential_directory` (loaded from `MODERATION_CREDENTIAL_DIRECTORY` by default) to the absolute path of a trusted, read-only mount. Set `credential_ref` and `credential_revision` in the named profile. For example, reference `sightengine` and revision `r1` read `sightengine.r1.json`, a JSON object containing `api_user` and `api_secret`. The configuration cache stores the directory, reference, and revision. HTTP, Artisan, and workers need access to the same mount.

The `*_env` keys in the examples below let you explicitly select environment variables. For mounted credentials, remove those keys and use their names without the `_env` suffix in the file. Each file is limited to 16 KiB. On Unix, use permissions such as `0400` or `0440` and keep the directory and files under deployment control. Add a new revision file when rotating keys and retain revisions referenced by outstanding tasks. To revoke a file, remove it and restart processes holding an older driver instance. Applications can bind `dyn ModerationCredentialProvider` in the container to resolve references and revisions through an existing secret service.

## Alibaba Cloud text {#aliyun-text}

Set `moderation.drivers.aliyun_comments` to a profile with `driver: "aliyun"`, `region: "cn-shanghai"`, `text_service: "comment_detection"`, `access_key_id_env`, and `access_key_secret_env`. The last two fields contain environment variable names. If using STS, set `security_token_env` as well. Select the profile through `Moderation::driver("aliyun_comments")` and call `text`, or make it the default with `MODERATION_DRIVER=aliyun_comments`.

The adapter uses `TextModeration` in `Green/2022-03-02` and accepts up to 600 Unicode characters. High risk maps to `Block`, and named low or medium risk maps to `Review`. An explicit empty label list with no risk yields `Allow` for text coverage. Unrecognized labels are retained in the findings. [Alibaba Cloud Text Moderation 2.0](https://help.aliyun.com/zh/document_detail/434034.html)

To use a language hint, list the tags supported by the chosen service in `text_languages`. A hint outside this profile is rejected before the request. Without a hint, the selected service receives the original text directly.

## Alibaba Cloud images {#aliyun-image}

For stored images, configure `image_service` such as `baselineCheck`, select a private S3 `media_disk`, and set `media_origin` to the HTTPS download origin for that disk. Pass `ImageInput::stored(StoredMedia::new(disk, key, version_id)?)` to the named driver. `version_id` must be an immutable S3 object version. The driver sends Alibaba Cloud a five-minute URL signed for that version. A mismatched origin, disk, or unversioned object is rejected before submission.

This operation supplies visual coverage. Frame samples and ambiguous `nonLabel` responses count as partial coverage. When the response contains confidence scores, `Assessment::confidence` returns the original value on a 0–100 scale. Use the coverage field when your policy also requires OCR. [Alibaba Cloud ImageModeration](https://help.aliyun.com/zh/document_detail/2528742.html)

## Tencent Cloud text {#tencent-text}

For a named profile, set `driver: "tencent"`, the `region` and `biz_type` configured in Tencent Cloud, and the environment variable names `secret_id_env` and `secret_key_env`. Add `security_token_env` for temporary credentials. The adapter calls standard `TextModeration` with UTF-8 Base64 text and accepts up to 10,000 Unicode characters.

Chinese hints map to `zh`. Set `english_text: true` once Tencent Cloud has enabled the English option for your account. An English hint then uses `en` for pure English text. The adapter checks that the returned policy matches the configured `biz_type`. A detail can increase the severity of the overall recommendation, and an unrecognized label cannot lead to `Allow`. [Tencent Cloud TextModeration](https://cloud.tencent.com.cn/document/api/1124/51860)

## Tencent Cloud images {#tencent-image}

Add `image_biz_type` with the standard image policy configured in Tencent Cloud. A profile can use images alone or in combination with text. The standard `ImageModeration` adapter sends inline image bytes as Base64 (at least 16 bytes before encoding and under 10 MB after encoding), or a five-minute URL pinned to a private S3 object version. For stored images, set `media_disk` and `media_origin` to the authorized HTTPS storage origin.

The adapter preserves risk findings from visual and OCR results with their original scores. Tencent Cloud may sample frames or split long images. Its synchronous response provides no proof of full coverage, so a provider `Pass` remains inconclusive. OCR text, matched keywords, and temporary links stay out of reported results. [Tencent Cloud ImageModeration](https://cloud.tencent.com/document/product/1125/53273)

## Sightengine text {#sightengine-text}

Set `driver: "sightengine"`, `api_user_env` and `api_secret_env` in a named profile. Supply the language codes enabled for that profile in `text_languages`. For `text_mode: "rules"`, configure `text_categories`, such as `["profanity", "personal"]`. For `text_mode: "ml"`, set `text_models` and a `text_review_score` from 0 to 1. The `self-harm` model requires `text_languages: ["en"]`.

The adapter preserves the rule or model label for each finding and the original score for ML findings. Findings at or above the configured threshold require review. Missing results for a configured category or class leave text coverage partial. The adapter accepts up to 10,000 Unicode characters and keeps matched text and API credentials out of results and logs. [Rules](https://sightengine.com/docs/text-moderation-rule-based) · [ML models](https://sightengine.com/docs/text-moderation-ml-models)

## Sightengine images {#sightengine-image}

Set `image_models` to the models needed for a profile and `image_review_score` to the visual review threshold from 0 to 1. Supported models are `nudity-2.1`, `violence`, `gore-2.0`, and `text-content-2.0`. For OCR, set `image_text_categories` to the categories you want checked, such as `["email", "insult"]`.

Inline images are uploaded as multipart. Stored images use `media_disk` and `media_origin` to provide a short-lived HTTPS URL pinned to the private S3 object version. The adapter keeps the highest visual score per label across returned frames and reports OCR categories without matched text. Results with unproven coverage remain partial and cannot authorize an Allow decision. [Sightengine image API](https://sightengine.com/docs/reference) · [OCR](https://sightengine.com/docs/ocr-text-moderation-in-images-2.0)

## Hive text {#hive-text}

For a Hive V2 Enterprise text project, set `driver: "hive"`, the numeric `text_project` ID, and `token_env` with the environment variable name for that project's token. Set `text_classes` to the categories your application requires and `text_review_severity` to 1, 2, or 3. To allow explicit language hints, list the project languages in `text_languages`. Requests without a hint use Hive's language detection.

The adapter limits input to 1,024 UTF-16 code units. It checks the project, model type, and task ID in the response. A class with score -1 or a missing required class leaves coverage partial. The largest severity for each class is used across all returned sections. Pattern-match findings name the source category and omit matched words and personal data. [Hive V2 text API](https://docs.thehive.ai/docs/classification-text)

## Hive images {#hive-image}

Set `image_project` to the numeric ID of the V2 Enterprise visual project and `image_token_env` to the environment variable holding that project's token. Set `image_model_type` to the value reported by that project, `image_classes` to the positive classes to evaluate, and `image_review_score` to the review threshold from 0 to 1. A profile can enable visual and text moderation with separate project IDs and tokens.

Inline images use multipart upload. For stored images, `media_disk` and `media_origin` provide a short-lived HTTPS URL pinned to a private S3 object version. The adapter keeps the highest score per configured class across the sampled results. Coverage remains partial when the service cannot establish that it processed the entire image sequence. You can also configure a separate Hive OCR project. [Hive visual moderation](https://docs.thehive.ai/docs/visual-content-moderation)

## Hive OCR {#hive-ocr}

Set `ocr_project`, `ocr_token_env`, `ocr_classes`, and `ocr_review_severity` in a named `hive` profile. The OCR project uses Hive's `OCR_MODERATION` model. Call `image()` on that profile and request `Modality::Ocr` when checking its decision. To require both visual and OCR results, call the two named projects and check both decisions.

Hive can establish complete coverage for a confirmed still image with one frame at timestamp zero and results for every required class in each detected block. Animation or incomplete evidence remains partial. The adapter preserves category-level findings and their original severity, without returning recognized text, matched values, or coordinates. [Hive OCR moderation](https://docs.thehive.ai/reference/ocr-moderation)

## Alibaba Cloud video {#aliyun-video}

Set `video_service` in a named `aliyun` profile. `videoDetection` supports Shanghai, Beijing and Hangzhou, and `videoDetectionByVL` supports Shanghai and Beijing. Set `media_disk` and `media_origin` for a versioned private S3 object. The driver signs a five-minute HTTPS download URL for the requested version.

`submit_video()` returns a provider task ID. Keep it with the original profile and query it through `query_video()`. Alibaba Cloud recommends waiting at least 30 seconds before the first query and retains results for 24 hours. If the submission response is lost, resolve `SubmissionUnknown` before creating another task. Remote cancellation is only documented for live streams.

A finished task can still report partial visual, speech or OCR coverage. `Assessment::evidence_span()` locates sampled frames or audio findings in milliseconds from the start of the original video. Recognized text and temporary evidence links are omitted. [Alibaba Cloud video API](https://help.aliyun.com/en/document_detail/2505810.html)

## Tencent Cloud video {#tencent-video}

Set `video_biz_type` to the standard on-demand policy for the named `tencent` profile, and configure `media_disk` and `media_origin` for private, versioned S3 media. This uses VM API `2021-09-22` with `Type=VIDEO`.

Submit with `submit_video()` and query the returned task ID through the same profile. `cancel_video()` handles pending tasks. It checks the task's type and policy before sending a cancellation, and the provider can refuse if processing has already started. When submitting a task gives `SubmissionUnknown`, reconcile it before submitting again.

The driver reports in-response frame, speech and OCR findings in milliseconds on the source timeline. The result retains partial coverage when the API cannot prove that the whole media and its required modalities were reviewed. [Create task](https://cloud.tencent.com/document/api/1265/80017) · [Query](https://cloud.tencent.com/document/product/1265/80016) · [Cancel](https://cloud.tencent.com/document/api/1265/80018)

## Sightengine video {#sightengine-video}

Set `video_models` and `video_review_score` for a named `sightengine` profile. Use `video_text_categories` when the OCR model is enabled. `video_max_duration` defaults to 1,800 seconds and accepts values up to 3,600 seconds where your plan permits. Configure `media_disk` and `media_origin` for the versioned private source.

`submit_video()` returns a media ID. Use the same profile to query the job or stop it. A finished job can reflect a duration limit or sampled frames. The async path reports only the visual and OCR evidence it received, and speech coverage remains unavailable. The HTTP client's diagnostics redact the service credentials from the outgoing query and stop body. [Async video](https://sightengine.com/docs/moderate-stored-video-asynchronously) · [Job operations](https://sightengine.com/docs/interact-with-video-moderation-jobs)

### Account-level video callbacks {#sightengine-callbacks}

Enable `content-moderation` and `webhook-client`. Point the Sightengine account’s callback URL at a route registered through Webhook Client. Use a separate route profile for each supplier account and set its `signature_validator` to `moderation-sightengine`. Set `moderation_callback_credential` to an object such as `{"name":"sightengine-callback","revision":"r2"}` and keep `signing_secret` in the mounted `sightengine-callback.r2.json` file. Select the Webhook Client store and processing job for the application.

For a rotation, `moderation_previous_callback_credentials` can list up to three earlier references. Remove retired references from the route configuration when revoking them. Verification checks the raw body and the timestamp within five minutes. Sightengine’s per-request callback URLs send unsigned notifications, so use the account-level URL for this signed route. [Sightengine callback signatures](https://sightengine.com/docs/callback-signature)

For durable reviews, register a `SightengineCallbackProcessor` with `QueuedWebhookJob::register_with_database_deferred_processing`. Give the processor the review store, the account alias used by the pinned profile, and the Webhook Client scope, profile name and revision. Early callbacks stay in Webhook Client's deferred processing until a matching provider task ID is available. If the application already knows the reception ID, it can use `QueuedWebhookJob::wake_deferred` to resume it sooner.

The processor links a matching terminal notification to its review and makes the original provider task due for confirmation. `moderation:recover` then queues the authenticated query. The review continues to apply its stored query budget, deadline and required coverage.

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

For delayed queries, declare the full profile under `moderation.profile_revisions.<name>.<revision>`. Add `account`, `policy_revision`, `credential_ref`, `credential_revision` and a nonempty `required_modalities` array using `text`, `visual`, `speech` or `ocr`. Record the account or project alias you provisioned with that credential. Use a new profile revision whenever the endpoint, model, policy, or credential revision changes. Keep earlier revisions in configuration until their tasks finish.

Call `Moderation::pin_profile(name, revision)` before submission and resolve the driver with `Moderation::driver_for_revision(&profile)`. Persist the returned profile locator with the remote `TaskId` and the original object version. Use the locator to resolve the driver again when querying. Recovery fails if the configuration was edited in place or removed, or the recorded credential can no longer be resolved. A pending task, execution failure, or incomplete required coverage cannot authorize publication.

Check the requested modalities with `Assessment::decision`. Review and block can retain risk evidence when results are incomplete. A successful result with partial speech coverage cannot produce `Allow` for a policy that requires speech.

## Persisting a video review {#video-review-storage}

Run `./artisan migrate` after enabling the package. To publish the two migration sources into the application, run `./artisan vendor:publish --tag=moderation-migrations`.

Create a `ModerationStore` with the database connection and stable application and tenant identifiers shared by HTTP and worker processes. Build a `VideoReviewRequest` from an idempotency key, the business content ID and revision, the original `StoredMedia`, a pinned profile, and a timeout in seconds. Call `stage_video` to record the pending review and its video check in one transaction. Repeating the same request returns the original record. Reusing the key within the same scope for different inputs produces a conflict. Use `find` to load the record by review ID from the same scope.

After staging, build a `VideoReviewService` from the store and the application’s `ModerationManager`. Call `submit(review_id)` from the worker or application. The service claims the check in the database, resolves its original credential revision, and sends the original object version. A concurrent caller can see `Submitting`. A confirmed response stores the provider’s task ID. If the response is ambiguous or the worker disappears after claiming, a later call records `Unknown` once the lease expires. Reconcile that task with the provider before deciding whether to create a new review. A review closed by its deadline stays expired.

Call `VideoReviewService::query(review_id)` to advance a confirmed task. Set `query_interval_seconds` and `query_max_attempts` on the pinned profile to control polling. They default to 30 seconds and 120 attempts, with each value limited to 1–300. The service stores the next query time, lease and attempt count in the database, so these limits apply across restarts. Normal pending responses use the configured interval. Temporary errors use bounded backoff.

Read the normalized evidence with `VideoReview::assessment()` and the policy decision with `VideoReview::decision()`. The decision is available after all required modalities have complete coverage. A remote task can finish with partial coverage, in which case the review stores the evidence and reports the coverage failure.

## Recovering scheduled reviews {#review-recovery}

Schedule `moderation:recover` to run every minute for each application and tenant you manage. Use a shared database and queue so schedulers and workers see the same review state. The command claims up to `--limit` due reviews (100 by default, 1–500), and queues their local IDs and versions.

```sh
./artisan moderation:recover app tenant --connection=database --queue=moderation
./artisan queue:work database --queue=moderation
```

Use `--database` to select a named review database. Give the worker's queue a reservation or visibility timeout longer than the recovery job’s 125-second timeout. The database keeps the next recovery time. A lost message may be queued again after its 60-second dispatch lease expires. A delivery for an earlier review version leaves the current review unchanged.
