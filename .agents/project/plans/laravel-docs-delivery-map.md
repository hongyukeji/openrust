# Laravel 13.x 文档与正式版交付对照

上游以[固定文档导航](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/documentation.md)为准。下表按上游导航排列，开发顺序和验收单元见[分批交付计划](../plan.md)。需确认表示生态移植范围需要独立确认。编号表示计划。

对应路径在[英文](../../../docs/en/README.md)、[简体中文](../../../docs/zh_CN/README.md)和[繁體中文](../../../docs/zh_TW/README.md)导航中相同。固定导航共 103 个章节入口，对应 102 个上游正文路径。Browser Tests 与 Dusk 共用一个路径。API Documentation 是外部参考入口。

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
| [Fortify](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/fortify.md) | [packages/fortify.md](../../../docs/en/packages/fortify.md) | [API-F01／02，第一代](../plan.md#unit-api-f01) |
| [Folio](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/folio.md) | [packages/folio.md](../../../docs/en/packages/folio.md) | 需确认 |
| [Head](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/head.md) | [packages/head.md](../../../docs/en/packages/head.md) | 需确认 |
| [Homestead](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/homestead.md) | [packages/homestead.md](../../../docs/en/packages/homestead.md) | 需确认 |
| [Horizon](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/horizon.md) | [packages/horizon.md](../../../docs/en/packages/horizon.md) | 需确认 |
| [Mix](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/mix.md) | [packages/mix.md](../../../docs/en/packages/mix.md) | 需确认 |
| [Octane](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/octane.md) | [packages/octane.md](../../../docs/en/packages/octane.md) | 需确认 |
| [Passport](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/passport.md) | [packages/passport.md](../../../docs/en/packages/passport.md) | [Passport-01 至 09，第一代](../plan.md#first-generation-api) |
| [Pennant](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pennant.md) | [packages/pennant.md](../../../docs/en/packages/pennant.md) | 需确认 |
| [Pint](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pint.md) | [packages/pint.md](../../../docs/en/packages/pint.md) | 需确认 |
| [Precognition](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/precognition.md) | [packages/precognition.md](../../../docs/en/packages/precognition.md) | [API-P01，第一代](../plan.md#unit-api-p01) |
| [Prompts](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/prompts.md) | [packages/prompts.md](../../../docs/en/packages/prompts.md) | 需确认 |
| [Pulse](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/pulse.md) | [packages/pulse.md](../../../docs/en/packages/pulse.md) | 需确认 |
| [Reverb](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/reverb.md) | [packages/reverb.md](../../../docs/en/packages/reverb.md) | [08-02](../plan.md#unit-08-02) |
| [Sail](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sail.md) | [packages/sail.md](../../../docs/en/packages/sail.md) | 需确认 |
| [Sanctum](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md) | [packages/sanctum.md](../../../docs/en/packages/sanctum.md) | [Sanctum-01 至 03](../plan.md#sanctum-与-api-安装) |
| [Scout](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scout.md) | [packages/scout.md](../../../docs/en/packages/scout.md) | 需确认 |
| [Socialite](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/socialite.md) | [packages/socialite.md](../../../docs/en/packages/socialite.md) | [API-S01，第一代](../plan.md#unit-api-s01) |
| [Telescope](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/telescope.md) | [packages/telescope.md](../../../docs/en/packages/telescope.md) | 需确认 |
| [Valet](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/valet.md) | [packages/valet.md](../../../docs/en/packages/valet.md) | 需确认 |

## 参考与 OpenRust 扩展

- [Laravel API Documentation](https://api.laravel.com/docs/13.x) 是上游 PHP API 参考。OpenRust API 随对应模块维护。
- [gRPC](../../../docs/en/digging-deeper/grpc.md) 是 OpenRust 扩展，参考固定 Goravel 配置与装配，对应 [08-03](../plan.md#unit-08-03)。

API 相关扩展自 2026-11-19 起纳入第一代。OIDC RP 与双应用 SSO 分别见 [API-O01／02](../plan.md#unit-api-o01)，协议与 Passport／Socialite 分开验收。包页面为空不表示实现完成；正式发布前必须闭合第一代正文与 API 参考。其余生态条目按各自实际用途核定，不因本次授权自动全部纳入。

## 生态导航的支持边界（2026-11-20）

第一代已实现功能的正文与各单元验收对应。其余 26 个生态／工具主题现已逐页说明支持状态和已有原生入口：5 项为部分原生能力（Agent 开发指引、前端视图、浏览器验收、控制台交互、Vite 字体 manifest），21 项没有对应完整移植。保留 Laravel 导航用于对照，不将这些页面计为包实现或新增交付承诺。语言首页提供实际安装、初始化、认证、OIDC、部署与版本入口。

三语导航、正文非空和相对 Markdown 引用由 `docs/tests/check_content.mjs` 检查；`check_scope.mjs` 在站点根／子路径逐页浏览三语导航。VitePress 不再把空白页面自动填成标题，空导航章节构建失败。发布范围的 API 参考和最终快照仍需单独核对，不能因消除空页就宣布 V16／V17 全部闭合。

## 教程消费者执行状态（2026-11-24）

以上章节映射用于安排工作，不等于每个官方示例已通过。逐例确认实际类型、公开调用、默认值、失败路径和请求／任务隔离；直接提取正式三语示例作为消费者，涉及HTTP／Artisan／持久化时验证真实入口。只把已核定子范围记为通过，不以同名搜索或旧回归替代教程验收。

| 章节子范围 | 状态与边界 | 证据 |
| --- | --- | --- |
| URL Generation：列明教程 | 请求验签、完整URL、默认值、Uri与优先级消费者已核定；保留APP_URL优先与previous同源差异，不等于整个类型API完全无差异 | [请求教程核对](../audits/docs-tutorials/README.md) |
| Validation：列明教程子范围 | 消息／字段名、具名包、FormRequest、未知字段、两类生成器、闭包／隐式规则、翻译和AnyOf已验收；ForEach／其余builder／setter／email模式未关闭 | [请求教程核对](../audits/docs-tutorials/README.md) |
| Settings：既定类型化设置与单机／集群协调 | 功能已闭合；发布环境矩阵保留待验，不重开功能任务 | [最终闭合复核](../audits/settings-coordination/README.md) |
| 其他章节及上述章节的剩余示例 | 依现有细分单元接续；未取得教程消费者证据的，不新增已通过结论 | [统一差异账本](framework-parity-data-2026-11-22.json) |

2026-11-24 URL第二组：Url::full与固定PHP实际输出对齐；Request实例／门面支持验签过滤回调，实际HTTP覆盖过期链接的bootstrap渲染及默认回退。13项专项／回归通过，严格Clippy／fmt／三语构建通过。整章其他教程逐项继续核对；APP_URL来源优先和previous同源限制保留明确差异，见[逐组记录](../audits/docs-tutorials/README.md)。

2026-11-24 Validation第三组：请求实例／门面、Validator与FormRequest已贯通消息和字段名；三语实际示例提取、真实JSON／Web具名会话／Precognition及关联回归32项通过，严格Clippy／fmt／三语构建通过。未知字段拒绝、email扩展模式及整章规则仍保持独立待核对，不能标为全章完成。

2026-11-24 URL第四组：补齐相对优先级配置，实际HTTP观察默认值→模型绑定→后置顺序，14项模型／中间件／URL回归（真实MySQL8.4.13）及补强Uri工厂／组件通过。URL章各教程的入口、消费者和既有APP_URL／同源previous差异已逐节列在[教程记录](../audits/docs-tutorials/README.md)，不等同于完整类型审计或完全行为无差异。

2026-11-24 Validation第五组：未知字段拒绝的表单／应用策略、嵌套与通配符、正文/查询/文件边界已接通；实测修复Blade @error/@else的500。独立最终22目标121项（MySQL8.4.13、三语十二段提取）通过，严格Clippy/fmt/三语构建浏览器通过。整章未关闭：make:request仍为planned命令，Closure／Implicit对象／Rule builders与email扩展模式列为明确接续项，见[当前逐节边界](../audits/docs-tutorials/README.md)。

2026-11-24 Validation第六组：make:request已从真实CLI占位失败补成正式生成器，含默认拒绝、命名空间/force/模板发布与保护。三语6条实际Artisan命令生成相同代码，typed动作真实HTTP403/422/201通过；5项CLI回归及严格Clippy/fmt/三语构建通过。第五组make:request缺口关闭，其余规则对象／builder接续。

2026-11-24 Validation第七组：Rule::custom支持字段级闭包，对象is_implicit控制缺失/nullable/空值调度；修正字面点字段名，复用原管线。固定PHP10组调度与真实HTTP、MySQL8.4.13等89项通过，三语18段提取及严格Clippy/fmt/构建浏览器通过。make:rule和其余对象setter/首选语言/builder等继续逐入口收尾。

2026-11-24 Validation第八组：make:rule／--implicit与两类可发布模板已接通；修复请求／规则模板合法类型名与导入、标准库名称冲突。8项实际CLI、三语12条命令生成文件、2项真实TCP生成代码消费者通过；清理后严格workspace Clippy/fmt/三语构建通过。生成器子范围闭合，逐失败语言、规则setter、builder和email扩展模式继续按[教程记录](../audits/docs-tutorials/README.md)逐项处理，整章不提前关闭。

2026-11-24 Validation第九组：RuleFailure通过显式RuleContext借用提供translate／translate_choice，当场解析首选语言与复数，保持原JSON错误包和默认消息；固定PHP包含语言切换时序、回退与嵌套替换参数的输出已用24并发真实TCP回放。20项验证回归＋三语21段直接提取的3项HTTP消费者通过，严格Clippy/fmt/三语构建通过。其余setter／builder／email差异继续，未宣称整章关闭。

2026-11-24 Validation第十组：Rule::any_of／AnyOf、标量及字段备选组闭合。固定PHP34组消息／值组合，真实TCP混合与递归组、工厂消息继承与短路、typed multipart成功／超限、MySQL8.4.13写连接存在性验证均通过。4目标28项＋三语24段正式示例3项消费者=31项，严格Clippy/fmt/三语构建浏览器通过。ForEach编译展开、其余builder、可变setter和email模式仍按逐项清单接续，未关闭整章。

2026-11-24 Validation第十一组：ForEach／NestedRules／CompilableRules及实际编译生命周期已接通，修复sometimes上下文、转义路径、文件替换与动态投影关联问题。固定PHP11组＋sometimes4组、真实HTTP/Precognition/multipart与MySQL8.4.13组合36项通过；三语直接提取消费者3项、最终严格Clippy/fmt/三语构建浏览器通过。新增[逐节对账表](../audits/docs-tutorials/section-map.json)，全文复核新发现safe、Enum/In、密码策略缺口一并开放，接续实现，不结束本轮任务。

2026-11-24 Validation第十二组：safe结果筛选／合并／集合／迭代及ValidatedData契约接通，文件句柄与数字追加状态保留；真实TCP含FormRequest/multipart、三语提取及回归11项0忽略通过，实际Artisan帮助与严格Clippy/fmt/三语构建浏览器通过。其余规则类型与setter等继续，整章未关闭。

2026-11-24 Validation第十三组：Enum/In/NotIn由空壳补齐，140组固定PHP行为由真实TCP回放，含过滤优先与值列表成员边界。11项回归＋三语消费者3项、实际Artisan、严格Clippy/fmt/三语构建浏览器通过。剩余fluent参数过滤等关联缺口继续，整章未关闭。

2026-11-24 Validation第十四组：四类链式builder、五类参数化类型／Trait、条件规则与六类布尔回调对象接通。130＋50组固定数据与真实HTTP，42项回归＋三语消费者3项通过，额外MySQL8.4.13重叠30项与实际Artisan通过；严格Clippy/fmt/三语构建浏览器通过。setter、email、密码及新发现的数组消息索引占位符等继续，整章未关闭。

2026-11-24 Validation第十五组：DataAware/ValidatorAware可变setter与同一验证器安全句柄闭合，29组固定数据及实际HTTP验证顺序、变更、错误联动、取消／隔离与原始文件句柄。当前HEAD隔离组合MySQL8.4.13回归47项＋三语消费者3项通过；恢复清理后严格Clippy/fmt/文档浏览器及实际Artisan通过。基础工厂、where入口、email、密码及数组消息位置继续，整章仍开放。

2026-11-24 Validation第十六组：命名规则工厂、Confirmed/CurrentPassword链式配置与ImageFile类型闭合；101行为＋78参数固定对照、真实TCP／认证／UDP及MySQL8.4.13六目标37项通过，三语正式消费者、严格Clippy/fmt/文档浏览器及实际Artisan通过。数据库where、email、密码、消息位置和全章复核继续。

2026-11-24 Validation第十七组：数据库where回调／数组同名入口闭合，50组固定SQLite参考由MySQL8.4.13真实TCP重放；三语原示例直接提取并实际查询，六目标44项0忽略与严格门禁、HTTP／Artisan通过。email、密码、消息位置及全章复核继续。
