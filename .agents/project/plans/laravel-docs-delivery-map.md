# Laravel 13.x 文档与正式版交付对照

上游以[固定文档导航](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/documentation.md)为准。下表按上游导航排列，开发顺序和验收单元见[分批交付计划](../plan.md)。需确认表示生态移植范围需要独立确认。编号表示计划。

对应路径在[英文](../../../docs/en/README.md)、[简体中文](../../../docs/zh-Hans/README.md)和[繁體中文](../../../docs/zh-Hant/README.md)导航中相同。固定导航共 103 个章节入口，对应 102 个上游正文路径。Browser Tests 与 Dusk 共用一个路径。API Documentation 是外部参考入口。

## Prologue

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Release Notes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/releases.md) | [prologue/releases.md](../../../docs/en/prologue/releases.md) | [08-04](../plan.md#unit-08-04) |
| [Upgrade Guide](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/upgrade.md) | [prologue/upgrade.md](../../../docs/en/prologue/upgrade.md) | [08-04](../plan.md#unit-08-04) |
| [Contribution Guide](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/contributions.md) | [prologue/contributions.md](../../../docs/en/prologue/contributions.md) | [08-04](../plan.md#unit-08-04) |

## Getting Started

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Installation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/installation.md) | [getting-started/installation.md](../../../docs/en/getting-started/installation.md) | [00-01](../plan.md#unit-00-01)、[08-04](../plan.md#unit-08-04) |
| [Configuration](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md) | [getting-started/configuration.md](../../../docs/en/getting-started/configuration.md) | [00-01](../plan.md#unit-00-01) |
| [Agentic Development](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/ai.md) | [getting-started/ai.md](../../../docs/en/getting-started/ai.md) | 需确认 |
| [Directory Structure](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/structure.md) | [getting-started/structure.md](../../../docs/en/getting-started/structure.md) | [00-01](../plan.md#unit-00-01) |
| [Frontend](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/frontend.md) | [getting-started/frontend.md](../../../docs/en/getting-started/frontend.md) | 需确认 |
| [Starter Kits](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/starter-kits.md) | [getting-started/starter-kits.md](../../../docs/en/getting-started/starter-kits.md) | 需确认 |
| [Deployment](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md) | [getting-started/deployment.md](../../../docs/en/getting-started/deployment.md) | [00-03](../plan.md#unit-00-03)、[08-04](../plan.md#unit-08-04) |

## Architecture Concepts

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Request Lifecycle](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/lifecycle.md) | [architecture-concepts/lifecycle.md](../../../docs/en/architecture-concepts/lifecycle.md) | [00-01](../plan.md#unit-00-01) |
| [Service Container](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/container.md) | [architecture-concepts/container.md](../../../docs/en/architecture-concepts/container.md) | [01-01](../plan.md#unit-01-01)、[01-02](../plan.md#unit-01-02) |
| [Service Providers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/providers.md) | [architecture-concepts/providers.md](../../../docs/en/architecture-concepts/providers.md) | [01-01](../plan.md#unit-01-01)、[01-02](../plan.md#unit-01-02) |
| [Facades](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/facades.md) | [architecture-concepts/facades.md](../../../docs/en/architecture-concepts/facades.md) | [01-01](../plan.md#unit-01-01) |

## The Basics

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Routing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md) | [the-basics/routing.md](../../../docs/en/the-basics/routing.md) | [00-02](../plan.md#unit-00-02)、[04-01](../plan.md#unit-04-01)、[04-03](../plan.md#unit-04-03)、[04-07](../plan.md#unit-04-07)、[05-05](../plan.md#unit-05-05)、[06-01](../plan.md#unit-06-01) |
| [Middleware](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/middleware.md) | [the-basics/middleware.md](../../../docs/en/the-basics/middleware.md) | [04-03](../plan.md#unit-04-03)、[04-04](../plan.md#unit-04-04) |
| [CSRF Protection](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/csrf.md) | [the-basics/csrf.md](../../../docs/en/the-basics/csrf.md) | [04-04](../plan.md#unit-04-04) |
| [Controllers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/controllers.md) | [the-basics/controllers.md](../../../docs/en/the-basics/controllers.md) | [04-02](../plan.md#unit-04-02) |
| [Requests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md) | [the-basics/requests.md](../../../docs/en/the-basics/requests.md) | [04-02](../plan.md#unit-04-02)、[04-06](../plan.md#unit-04-06) |
| [Responses](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/responses.md) | [the-basics/responses.md](../../../docs/en/the-basics/responses.md) | [04-02](../plan.md#unit-04-02)、[06-01](../plan.md#unit-06-01) |
| [Views](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/views.md) | [the-basics/views.md](../../../docs/en/the-basics/views.md) | [03-07](../plan.md#unit-03-07)、[03-08](../plan.md#unit-03-08) |
| [Blade Templates](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md) | [the-basics/blade.md](../../../docs/en/the-basics/blade.md) | [03-07](../plan.md#unit-03-07)、[03-08](../plan.md#unit-03-08) |
| [Asset Bundling](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/vite.md) | [the-basics/vite.md](../../../docs/en/the-basics/vite.md) | 需确认 |
| [URL Generation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/urls.md) | [the-basics/urls.md](../../../docs/en/the-basics/urls.md) | [06-01](../plan.md#unit-06-01) |
| [Session](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/session.md) | [the-basics/session.md](../../../docs/en/the-basics/session.md) | [04-04](../plan.md#unit-04-04) |
| [Validation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md) | [the-basics/validation.md](../../../docs/en/the-basics/validation.md) | [04-05](../plan.md#unit-04-05)、[04-06](../plan.md#unit-04-06) |
| [Error Handling](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/errors.md) | [the-basics/errors.md](../../../docs/en/the-basics/errors.md) | [00-02](../plan.md#unit-00-02)、[01-06](../plan.md#unit-01-06)、[01-07](../plan.md#unit-01-07)、[03-07](../plan.md#unit-03-07)、[04-08](../plan.md#unit-04-08) |
| [Logging](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md) | [the-basics/logging.md](../../../docs/en/the-basics/logging.md) | [01-03](../plan.md#unit-01-03)、[01-04](../plan.md#unit-01-04)、[01-05](../plan.md#unit-01-05) |

## Digging Deeper

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Artisan Console](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md) | [digging-deeper/artisan.md](../../../docs/en/digging-deeper/artisan.md) | [00-03](../plan.md#unit-00-03)、[06-08](../plan.md#unit-06-08) |
| [Broadcasting](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/broadcasting.md) | [digging-deeper/broadcasting.md](../../../docs/en/digging-deeper/broadcasting.md) | [08-01](../plan.md#unit-08-01)、[08-02](../plan.md#unit-08-02) |
| [Cache](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md) | [digging-deeper/cache.md](../../../docs/en/digging-deeper/cache.md) | [03-06](../plan.md#unit-03-06)、[03-09](../plan.md#unit-03-09) |
| [Collections](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/collections.md) | [digging-deeper/collections.md](../../../docs/en/digging-deeper/collections.md) | [02-08](../plan.md#unit-02-08)、[02-09](../plan.md#unit-02-09) |
| [Concurrency](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/concurrency.md) | [digging-deeper/concurrency.md](../../../docs/en/digging-deeper/concurrency.md) | [07-04](../plan.md#unit-07-04) |
| [Context](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/context.md) | [digging-deeper/context.md](../../../docs/en/digging-deeper/context.md) | [01-05](../plan.md#unit-01-05)、[06-04](../plan.md#unit-06-04) |
| [Contracts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/contracts.md) | [digging-deeper/contracts.md](../../../docs/en/digging-deeper/contracts.md) | [01-01](../plan.md#unit-01-01) |
| [Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/events.md) | [digging-deeper/events.md](../../../docs/en/digging-deeper/events.md) | [06-02](../plan.md#unit-06-02)、[T-04](../plan.md#unit-t-04) |
| [File Storage](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/filesystem.md) | [digging-deeper/filesystem.md](../../../docs/en/digging-deeper/filesystem.md) | [00-02](../plan.md#unit-00-02)、[03-01](../plan.md#unit-03-01)、[03-03](../plan.md#unit-03-03)、[04-09](../plan.md#unit-04-09)、[06-01](../plan.md#unit-06-01) |
| [Helpers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/helpers.md) | [digging-deeper/helpers.md](../../../docs/en/digging-deeper/helpers.md) | [07-01](../plan.md#unit-07-01) |
| [HTTP Client](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/http-client.md) | [digging-deeper/http-client.md](../../../docs/en/digging-deeper/http-client.md) | [07-02](../plan.md#unit-07-02) |
| [Images](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/images.md) | [digging-deeper/images.md](../../../docs/en/digging-deeper/images.md) | [07-05](../plan.md#unit-07-05) |
| [Localization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md) | [digging-deeper/localization.md](../../../docs/en/digging-deeper/localization.md) | [03-10](../plan.md#unit-03-10)、[06-08](../plan.md#unit-06-08) |
| [Mail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mail.md) | [digging-deeper/mail.md](../../../docs/en/digging-deeper/mail.md) | [06-06](../plan.md#unit-06-06) |
| [Notifications](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/notifications.md) | [digging-deeper/notifications.md](../../../docs/en/digging-deeper/notifications.md) | [06-06](../plan.md#unit-06-06) |
| [Package Development](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/packages.md) | [digging-deeper/packages.md](../../../docs/en/digging-deeper/packages.md) | [07-07](../plan.md#unit-07-07) |
| [Processes](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/processes.md) | [digging-deeper/processes.md](../../../docs/en/digging-deeper/processes.md) | [07-03](../plan.md#unit-07-03) |
| [Queues](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md) | [digging-deeper/queues.md](../../../docs/en/digging-deeper/queues.md) | [06-03](../plan.md#unit-06-03)、[06-04](../plan.md#unit-06-04) |
| [Rate Limiting](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/rate-limiting.md) | [digging-deeper/rate-limiting.md](../../../docs/en/digging-deeper/rate-limiting.md) | [04-07](../plan.md#unit-04-07) |
| [Search](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/search.md) | [digging-deeper/search.md](../../../docs/en/digging-deeper/search.md) | [07-06](../plan.md#unit-07-06) |
| [Strings](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/strings.md) | [digging-deeper/strings.md](../../../docs/en/digging-deeper/strings.md) | [07-01](../plan.md#unit-07-01) |
| [Task Scheduling](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md) | [digging-deeper/scheduling.md](../../../docs/en/digging-deeper/scheduling.md) | [06-05](../plan.md#unit-06-05) |

## Security

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Authentication](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/authentication.md) | [security/authentication.md](../../../docs/en/security/authentication.md) | [05-08](../plan.md#unit-05-08) |
| [Authorization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/authorization.md) | [security/authorization.md](../../../docs/en/security/authorization.md) | [05-09](../plan.md#unit-05-09) |
| [Email Verification](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/verification.md) | [security/verification.md](../../../docs/en/security/verification.md) | [06-07](../plan.md#unit-06-07) |
| [Encryption](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/encryption.md) | [security/encryption.md](../../../docs/en/security/encryption.md) | [03-04](../plan.md#unit-03-04) |
| [Hashing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/hashing.md) | [security/hashing.md](../../../docs/en/security/hashing.md) | [03-05](../plan.md#unit-03-05) |
| [Password Reset](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/passwords.md) | [security/passwords.md](../../../docs/en/security/passwords.md) | [06-07](../plan.md#unit-06-07) |

## Database

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Getting Started](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database.md) | [database/getting-started.md](../../../docs/en/database/getting-started.md) | [02-01](../plan.md#unit-02-01)、[02-02](../plan.md#unit-02-02) |
| [Query Builder](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queries.md) | [database/queries.md](../../../docs/en/database/queries.md) | [02-03](../plan.md#unit-02-03)、[02-04](../plan.md#unit-02-04) |
| [Pagination](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pagination.md) | [database/pagination.md](../../../docs/en/database/pagination.md) | [05-06](../plan.md#unit-05-06) |
| [Migrations](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/migrations.md) | [database/migrations.md](../../../docs/en/database/migrations.md) | [02-05](../plan.md#unit-02-05)、[02-06](../plan.md#unit-02-06) |
| [Seeding](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/seeding.md) | [database/seeding.md](../../../docs/en/database/seeding.md) | [02-07](../plan.md#unit-02-07)、[05-04](../plan.md#unit-05-04) |
| [Redis](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/redis.md) | [database/redis.md](../../../docs/en/database/redis.md) | [03-09](../plan.md#unit-03-09) |
| [MongoDB](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mongodb.md) | [database/mongodb.md](../../../docs/en/database/mongodb.md) | 需确认 |

## Eloquent ORM

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Getting Started](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md) | [eloquent/getting-started.md](../../../docs/en/eloquent/getting-started.md) | [05-01](../plan.md#unit-05-01) |
| [Relationships](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-relationships.md) | [eloquent/relationships.md](../../../docs/en/eloquent/relationships.md) | [05-02](../plan.md#unit-05-02) |
| [Collections](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-collections.md) | [eloquent/collections.md](../../../docs/en/eloquent/collections.md) | [05-02](../plan.md#unit-05-02) |
| [Mutators / Casts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md) | [eloquent/mutators.md](../../../docs/en/eloquent/mutators.md) | [05-03](../plan.md#unit-05-03) |
| [API Resources](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-resources.md) | [eloquent/resources.md](../../../docs/en/eloquent/resources.md) | [05-07](../plan.md#unit-05-07) |
| [Serialization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-serialization.md) | [eloquent/serialization.md](../../../docs/en/eloquent/serialization.md) | [05-03](../plan.md#unit-05-03) |
| [Factories](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-factories.md) | [eloquent/factories.md](../../../docs/en/eloquent/factories.md) | [05-04](../plan.md#unit-05-04) |

## AI

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [AI SDK](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/ai-sdk.md) | [ai/ai-sdk.md](../../../docs/en/ai/ai-sdk.md) | 需确认 |
| [MCP](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mcp.md) | [ai/mcp.md](../../../docs/en/ai/mcp.md) | 需确认 |
| [Boost](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/boost.md) | [ai/boost.md](../../../docs/en/ai/boost.md) | 需确认 |

## Testing

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Getting Started](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md) | [testing/getting-started.md](../../../docs/en/testing/getting-started.md) | [T-01](../plan.md#unit-t-01) |
| [HTTP Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/http-tests.md) | [testing/http-tests.md](../../../docs/en/testing/http-tests.md) | [T-02](../plan.md#unit-t-02) |
| [Console Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/console-tests.md) | [testing/console-tests.md](../../../docs/en/testing/console-tests.md) | [T-02](../plan.md#unit-t-02) |
| [Browser Tests](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/dusk.md) | [packages/dusk.md](../../../docs/en/packages/dusk.md) | 浏览器验证随批，Dusk 包需确认 |
| [Database](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md) | [testing/database-testing.md](../../../docs/en/testing/database-testing.md) | [T-03](../plan.md#unit-t-03) |
| [Mocking](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mocking.md) | [testing/mocking.md](../../../docs/en/testing/mocking.md) | [T-04](../plan.md#unit-t-04) |

## Packages

| Laravel 文档 | OpenRust 正文路径 | 交付单元 |
| --- | --- | --- |
| [Cashier (Stripe)](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/billing.md) | [packages/billing.md](../../../docs/en/packages/billing.md) | 需确认 |
| [Cashier (Paddle)](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cashier-paddle.md) | [packages/cashier-paddle.md](../../../docs/en/packages/cashier-paddle.md) | 需确认 |
| [Dusk](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/dusk.md) | [packages/dusk.md](../../../docs/en/packages/dusk.md) | 需确认 |
| [Envoy](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/envoy.md) | [packages/envoy.md](../../../docs/en/packages/envoy.md) | 需确认 |
| [Fortify](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/fortify.md) | [packages/fortify.md](../../../docs/en/packages/fortify.md) | 需确认 |
| [Folio](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/folio.md) | [packages/folio.md](../../../docs/en/packages/folio.md) | 需确认 |
| [Head](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/head.md) | [packages/head.md](../../../docs/en/packages/head.md) | 需确认 |
| [Homestead](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/homestead.md) | [packages/homestead.md](../../../docs/en/packages/homestead.md) | 需确认 |
| [Horizon](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/horizon.md) | [packages/horizon.md](../../../docs/en/packages/horizon.md) | 需确认 |
| [Mix](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mix.md) | [packages/mix.md](../../../docs/en/packages/mix.md) | 需确认 |
| [Octane](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/octane.md) | [packages/octane.md](../../../docs/en/packages/octane.md) | 需确认 |
| [Passport](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/passport.md) | [packages/passport.md](../../../docs/en/packages/passport.md) | 需确认 |
| [Pennant](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pennant.md) | [packages/pennant.md](../../../docs/en/packages/pennant.md) | 需确认 |
| [Pint](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pint.md) | [packages/pint.md](../../../docs/en/packages/pint.md) | 需确认 |
| [Precognition](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/precognition.md) | [packages/precognition.md](../../../docs/en/packages/precognition.md) | 需确认 |
| [Prompts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/prompts.md) | [packages/prompts.md](../../../docs/en/packages/prompts.md) | 需确认 |
| [Pulse](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pulse.md) | [packages/pulse.md](../../../docs/en/packages/pulse.md) | 需确认 |
| [Reverb](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/reverb.md) | [packages/reverb.md](../../../docs/en/packages/reverb.md) | [08-02](../plan.md#unit-08-02) |
| [Sail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sail.md) | [packages/sail.md](../../../docs/en/packages/sail.md) | 需确认 |
| [Sanctum](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md) | [packages/sanctum.md](../../../docs/en/packages/sanctum.md) | 需确认 |
| [Scout](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scout.md) | [packages/scout.md](../../../docs/en/packages/scout.md) | 需确认 |
| [Socialite](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/socialite.md) | [packages/socialite.md](../../../docs/en/packages/socialite.md) | 需确认 |
| [Telescope](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/telescope.md) | [packages/telescope.md](../../../docs/en/packages/telescope.md) | 需确认 |
| [Valet](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/valet.md) | [packages/valet.md](../../../docs/en/packages/valet.md) | 需确认 |

## 参考与 OpenRust 扩展

- [Laravel API Documentation](https://api.laravel.com/docs/13.x) 是上游 PHP API 参考。OpenRust API 随对应模块维护。
- [gRPC](../../../docs/en/digging-deeper/grpc.md) 是 OpenRust 扩展，参考固定 Goravel 配置与装配，对应 [08-03](../plan.md#unit-08-03)。
