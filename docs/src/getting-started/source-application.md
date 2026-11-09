# 源码应用与扩展指南

本页保留源码应用的运行、生成及公开扩展用法。命名空间调整与未闭合能力仍以各专题指南和实际验收为准。

## 当前实现与目标结构

根 `app/` 支持正常应用开发（Controller、Model、Provider）；`modules/<模块名>/` 是可选且可并存的业务组织，唯一综合示例为 `modules/example/`。根目录继续保留 Laravel 的 `bootstrap/`、`config/`、`database/`、`public/`、`resources/`、`routes/`、`storage/` 和 `tests/`，并保留 Rust 的 `src/`、`tools/` 和 Cargo。框架与应用同仓、同 workspace、只在 main 开发。

业务示例代码现位于 `modules/example/`，通过模块清单和默认 registered 激活器接入统一 Provider 内核。模块管理命令、完整 CRUD 与全部能力示例仍在实施，当前闭环不代表 laravel-modules 全矩阵已完成。目标与边界见 [ADR-001](../../../.agents/project/decisions/adr-001-framework-principles.md)。

## 查找应用命令

源码应用和生成项目都提供 POSIX `artisan` 脚本。它先切换到脚本所在目录，再执行应用入口。源码目录由 Cargo 运行，发布目录由脚本执行同目录的 `app`。脚本用 `exec` 交接进程，透传参数、信号和退出码。入口职责参照固定 Laravel 13.x 应用骨架的薄脚本。[Laravel artisan](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/artisan)

```bash
./artisan list
./artisan help migrate
./artisan module:list --help
./artisan help config show
```

`list` 显示框架、应用和已启用模块实际登记的命令。直接运行 `./artisan` 默认执行 `list`。仅传全局环境选项时也默认执行 `list`，例如 `./artisan --env=local`。带空格的命令名可逐词输入。

自定义命令由 Provider 注册。实现 `Command::about` 提供描述，实现 `Command::usage` 描述接受的参数和选项。帮助读取这些信息并沿用正常应用引导流程。

根应用与生成项目运行 `cargo run` 时，默认通过 `start` 在一个进程内启动 HTTP、已配置的队列 worker 和适用的定时任务。应用注册的 WebSocket 路由与 HTTP 共用端口，启用 `realtime` 并配置应用路由和授权后随 HTTP 可用。部署可用同一二进制的 `serve`、`queue work` 或 `schedule run` 分开运行各角色。

## 设计目标

缓存使用 [`openrust::cache`](../../../vendor/openrust/framework/src/cache/README.md) 的统一 API，标准生成应用通过 `cache-redis` 使用 Redis；memory/file 与自定义 driver 均需显式选择。开发期 `openrust dev` 会复用或启动本地 Compose Redis。

- **约定清晰，不依赖魔法。** 目录、命名、路由、迁移和配置行为按 Laravel 对应契约实现，并以 Rust 类型表达依赖。应用通过 `Container` 与 `ServiceProvider` 统一装配，Facade 对应能力仍按实施计划逐项验收。
- **每项能力都有明确入口。** 例如，输入校验使用 `Validated<T>`，授权使用 `Policy`，后台任务使用 `Job`。
- **常用能力开箱可用，驱动可替换。** v1 计划提供数据库队列、Redis 缓存、本地存储、SMTP 邮件和可扩展短信（含 Twilio 适配器），并支持显式选择 memory/file 缓存及可选的 S3 兼容存储。框架计划支持单机和集群部署；驱动通过 Cargo feature 与配置选择。
- **生成代码就是普通代码。** `openrust generate` 生成的内容均为可读、可编辑、可测试的 Rust 代码。
- **默认可测试。** 外部服务能力提供 fake 或可控本地服务验收路径；Compose 中的 Mailpit 服务支持真实本地 SMTP 验证。业务测试无需付费 SMTP/SMS 账号。

OpenRust 基于 `tokio`、`axum`、`tower` 和 `SeaORM` 构建，采用明确的 Rust 类型与常规项目结构。

## 从根目录运行

```bash
cargo run
# 可选覆盖监听端口
SERVER_PORT=4000 cargo run
```

根 `default-members = ["."]`、`default-run = "app"`，无参数 `cargo run` 启动 `app`。`app/` 编写应用代码，`modules/` 可选组织业务模块，`bootstrap/app.rs` 装配应用，`routes/` 注册路由；`config/`、`database/`、`public/`、`resources/`、`storage/` 和 `tests/` 都在根目录。

发布根应用时，在目标平台构建 `cargo build --release --locked --bin app`，按需启用应用 feature。将 `target/release/app` 和可执行的 `artisan` 放在同一个部署目录。使用数据库时在部署目录先运行 `./artisan migrate`，再用 `./app` 启动常驻进程。

当前综合示例 [`modules/example`](../../../modules/example/README.md) 是普通业务模块，展示如何通过公开 `openrust::...` API 接入业务；开发者可直接在根 `app/` 编写业务；需要模块组织时，在 `modules/<业务>/` 定义模块，经模块清单与统一 Provider 接入。它没有独立应用 manifest、启动入口或完整子应用骨架，也不需要额外 example feature。数据库、授权、队列等通过已有 Cargo feature 和根应用配置启用真实服务，缺必需依赖明确失败。

标准应用默认启用 Redis 缓存，需可用的 Redis 配置与服务；`/health` 与 `/ready` 提供探针。R2-M 模块清单、资源与迁移已有真实服务验收；管理写命令、完整发布和 CRUD 仍按当前计划推进。

```bash
cargo test -p openrust-framework -p openrust
cargo test --workspace
```

公开扩展及拓扑验收见 [consumers](../../../tests/consumers/README.md)，完整门禁见 [贡献说明](../../../CONTRIBUTING.md)。

## 用 Docker Compose 运行

从 OpenRust 源码仓库根目录运行以下命令。镜像包含同目录的 `app` 和 `artisan`。MySQL 8.4 和 Redis 就绪后，Compose 的一次性 `migrate` 服务执行 `./artisan migrate`。迁移成功后，应用容器直接运行 `app`，队列和适用的调度与 Web 共用一个进程。

```bash
docker compose up --build -d --wait
curl --fail http://127.0.0.1:3000/ready
docker compose logs -f app
```

默认将应用端口发布到宿主机的 `127.0.0.1:3000`。`SERVER_PORT` 修改宿主机端口。Compose 从部署目录的 `.env` 读取 `docker-compose.yml` 列明的部署变量，应用和迁移容器均使用 Compose 网络内的 MySQL 和 Redis。部署前设置独立的 `DB_PASSWORD`、正确的 `APP_URL` 和业务需要的密钥。

Dockerfile 使用 Cargo 默认 feature。需编译登录、视图和 WebSocket 时，可在 `.env` 设置 `OPENRUST_BUILD_FEATURES=auth-fortify,view,realtime`，并按应用配置填写认证密钥和允许的 WebSocket Origin。

### 宿主机数据与公开资源

相对路径以 Compose 文件所在目录为基准。每套部署使用自己的数据根。

| 宿主机路径 | 容器路径 | 用途 |
| --- | --- | --- |
| `${OPENRUST_DATA_DIR:-./docker-data}/storage` | `/app/storage` | 应用与迁移服务共享读写 |
| `${OPENRUST_DATA_DIR:-./docker-data}/mysql` | `/var/lib/mysql` | MySQL 数据 |
| `${OPENRUST_DATA_DIR:-./docker-data}/redis` | `/data` | Redis 持久文件 |
| `${OPENRUST_PUBLIC_DIR:-./public}` | `/app/public` | 应用公开资源，只读 |

启动时创建缺失的数据子目录。`app` 与 `migrate` 以 UID/GID `10001:10001` 运行。MySQL 和 Redis 分别使用其镜像内的 `mysql` 和 `redis` 账号。初始化会检查对应账号能否写入目录。只有不可写、为空且属主为 `0:0` 的挂载根会调整属主。迁入已有数据时，请保留各服务的数字 UID/GID，并确保目录对相应服务可写。`docker compose down` 移除容器和网络，宿主机数据目录可在下次启动时继续使用。

`OPENRUST_PUBLIC_DIR` 必须指向已有目录。挂载覆盖镜像的整个 `/app/public`，因此部署时需带上所需的 `index.html` 和公开资源。宿主机可更新这些文件，容器只能读取。该目录会通过应用公开访问，请只放可公开的内容。[Docker Compose 挂载说明](https://docs.docker.com/reference/compose-file/services/#volumes)

构建上下文排除 `.env`、SQLite 文件、默认的 `docker-data/`，以及任意层级名为 `storage`、`mysql` 或 `redis` 的目录。自定义数据根请使用构建上下文外的真实目录，三个数据子目录均应是实际目录。备份和迁移归档也应放在构建上下文外，避免随源码发送给构建器。[Docker 构建上下文](https://docs.docker.com/build/concepts/context/#dockerignore-files)

### 用现成镜像部署

在源码机选择一个与部署目标架构匹配的镜像标签。可用镜像仓库分发，也可构建后导出。

```bash
OPENRUST_IMAGE=openrust-app:deploy docker compose build app
docker image save --output ../openrust-app.tar openrust-app:deploy
```

将 `docker-compose.yml`、镜像归档及 `public/` 复制到部署主机。在部署目录创建 `.env`，设置 `OPENRUST_IMAGE=openrust-app:deploy` 和部署所需配置。使用镜像仓库时，直接拉取 `.env` 指定的镜像，可省略加载归档。

```bash
docker image load --input openrust-app.tar
```

如未随部署携带 `public/`，可在部署目录从同一镜像提取默认资源。提取后再启动服务。

```bash
(
  set -eu -o pipefail
  mkdir public
  docker run --rm --entrypoint tar openrust-app:deploy -C /app/public -cf - . |
    tar -xf - -C public
  test -r public/index.html
)
```

```bash
docker compose up --no-build -d --wait
```

### 从命名卷迁移

旧 Compose 使用 `storage`、`mysql`、`redis` 三个命名卷。切换前保留旧 Compose 文件、环境配置和镜像。先停外部流量和其他写入进程，正常停止应用，在 Redis 保存数据后正常停止 Redis 与 MySQL。确认旧容器使用的实际卷名及镜像版本。物理复制应沿用相同的 MySQL 与 Redis 镜像版本，跨版本迁移遵循相应数据库的升级流程。

```bash
project=openrust
old_compose=./docker-compose.named-volumes.yml
docker compose -p "$project" -f "$old_compose" stop app &&
  docker compose -p "$project" -f "$old_compose" exec -T redis redis-cli SAVE &&
  docker compose -p "$project" -f "$old_compose" stop --timeout 120 mysql redis
docker volume ls --filter "label=com.docker.compose.project=$project"
```

确认 Redis 返回 `OK`，且所有旧服务已正常停止后，再归档卷。以下 Linux 示例假定实际卷名沿用 Compose 的 `<项目>_<卷名>`。按部署填写备份目录和新的数据根，两者的上级目录须已存在，数据根须尚未使用。

```bash
backup=/srv/openrust-backups/volume-migration
data=/srv/openrust/data
mkdir -m 0700 "$backup" "$data" || exit 1
for name in storage mysql redis; do
  volume="${project}_${name}"
  docker volume inspect "$volume" >/dev/null || exit 1
  docker run --rm --mount "type=volume,src=$volume,dst=/source,readonly" \
    debian:bookworm-slim tar --numeric-owner -C /source -cf - . \
    > "$backup/$name.tar" || exit 1
  tar -tf "$backup/$name.tar" >/dev/null || exit 1
  mkdir "$data/$name" || exit 1
  sudo tar --numeric-owner --same-owner -xpf "$backup/$name.tar" \
    -C "$data/$name" || exit 1
done
```

核对归档和恢复目录的内容及数字属主。在新部署的 `.env` 设置 `OPENRUST_DATA_DIR=/srv/openrust/data`，沿用原数据库名和凭据，再用新版 Compose 启动。恢复流量前检查 `/ready`、业务记录、应用存储和 Redis 持久数据。保留归档和原命名卷，回退时先停止新版，再用旧配置启动原卷。若新版已接收写入，先确认如何保留这些新增数据。[Docker 命名卷备份与恢复](https://docs.docker.com/engine/storage/volumes/#back-up-restore-or-migrate-data-volumes)

在 macOS 的 Docker Desktop 上，应在实际部署的 Linux 主机执行上述物理恢复，或使用各服务支持的逻辑导出和导入流程。恢复后以容器中的实际运行账号核对文件属主和读写权限。

### 导出应用文件

在 Mac 上也可以用同一个 Dockerfile 导出 Linux 应用文件，导出目录中包含同级的 `app` 和 `artisan`。下面的命令目标是 x86_64，ARM64 对应 `linux/arm64`。导出的二进制使用构建镜像的 Linux ABI，直接部署前应在目标系统检查依赖并实际启动。

```bash
docker buildx build --platform linux/amd64 --target artifacts \
  --output type=local,dest=dist/linux-amd64 .
```

将文件部署到 Linux 主机后，在应用运行目录建立 `bootstrap/cache`，并让实际应用账号可写。首次启动会在其中生成服务清单。容器运行镜像已创建该目录。

这里的 Compose 文件用于根应用。默认构建上下文、数据目录 `./docker-data` 和公开资源 `./public` 均相对 Compose 文件解析。根应用已在原生 Ubuntu 24.04 x86_64 上实测部署、旧命名卷迁移和回退。

### 生成应用的容器部署

`openrust new` 生成应用自己的 `Dockerfile`、`docker-compose.yml` 和 `.dockerignore`。需要支持 `additional_contexts` 的 Docker Compose 与 BuildKit。应用的 `Cargo.toml` 和 Compose 文件通过相对路径使用生成时对应的 OpenRust 源码。复制或移动到另一台构建机时，保留应用与框架的相对目录关系。容器运行只需要构建产物、公开资源和运行配置。

在生成的应用目录中启动服务并检查就绪状态。

```bash
docker compose up --build -d --wait
docker compose exec app ./artisan list
curl --fail http://localhost:3000/ready
```

默认应用启动 MySQL 8.4 和 Redis，Artisan 完成数据库迁移后再启动 HTTP 服务。`--minimal` 仅启动应用和 Redis。`--db postgres` 使用 PostgreSQL 18。`--db sqlite` 在应用的持久化存储中创建容器专用的 SQLite 文件，并运行迁移。

应用容器以非 root 账号运行，数据目录默认为 `./docker-data`，`./public` 只读挂载，并须对应用账号可读。两者可通过 `OPENRUST_DATA_DIR` 和 `OPENRUST_PUBLIC_DIR` 配置。自定义数据目录时，将它放在应用与框架两个构建上下文之外，或在相应的 `.dockerignore` 排除该目录。服务默认绑定宿主的 `127.0.0.1:3000`，可用 `OPENRUST_HTTP_BIND` 和 `SERVER_PORT` 修改。数据库与 Redis 通过 Compose 内部网络访问。

部署 Fortify Web 前，从 `.env.example` 配置应用自己的 `.env`，设置生产环境、应用 URL 和数据库凭据。可在源码目录运行 `./artisan key:generate` 写入 `APP_KEY`，并用 `openrust key generate` 为模板声明的 JWT、签名链接和加密密钥项各生成独立值。Compose 只向容器传递 `environment` 中明确列出的变量，应用、迁移与后台任务使用同一套内部连接。构建上下文排除实际密钥文件。需要固定发布依赖时，将生成项目自己的 `Cargo.lock` 一并纳入版本控制，镜像会按该锁文件构建。

Fortify 提供 Web 认证动作。登录页等浏览器视图由应用通过 Fortify 的视图扩展点绑定。

### 开发重载

源码应用在 macOS 或 Linux 上使用 `./artisan dev`。根应用和 `openrust new` 生成的应用都可以从各自目录运行。开发环境需要支持 `--only-emit-events` 的 Watchexec，并将它放在 PATH。 [Watchexec 文档](https://github.com/watchexec/watchexec/blob/main/doc/watchexec.1.md)

```bash
./artisan dev --env local
```

保存源文件后，Cargo 先构建新版本，旧应用继续处理请求。成功构建后，重载器等待旧应用按正常 SIGTERM 生命周期退出，再启动新版本。监听范围包括路由、Rust 配置、模块、资源、Cargo 清单、锁文件、当前 dotenv 与本地路径依赖。构建和运行产物不会触发重载。普通启动仍可使用 `cargo run`。

## 编写业务与扩展

真实参考链为 `src/main.rs` → `bootstrap::Server::modules` → `modules::manifest` → 同一模块仓储与 Provider → 模块路由和 handler。根应用与新生成应用默认使用 `registered`：在 `modules/mod.rs` 显式登记模块即启用，移除登记即不挂载，Rust `mod` 本身只负责编译。默认不依赖状态 JSON；需要持久启停时显式选择 `file` 或 `file-live`。业务接入不修改框架 `src/`，详见[模块注册指南](../../../docs/src/modules/registration.md)和[唯一综合示例](../../../modules/example/README.md#新建自己的业务模块)。

综合示例的硬交付包括完整实体 CRUD，以及当前所有已实现公开能力的实际使用和 Provider/driver 扩展参考；目标业务边界为 modules/，模块之外保留框架通用结构和代码；示例专有迁移/种子/资源/协议/生成脚本及测试 fixture 留模块内，根只提供通用结构和公开挂载，不因示例保留 build.rs 或 build 依赖。当前正在补齐，基础常量路由不是 CRUD，文档或其他 consumer 的代码不代表模块已覆盖；实际代码、接入条件与测试缺口见 [能力覆盖表](../../../modules/example/README.md#当前能力覆盖与缺口)。

业务可以使用或新增第三方包，声明位置是根应用清单；普通 Rust 业务模块没有独立 Cargo 依赖边界，普通业务模块与根应用共享依赖解析和 feature 选择。已有 `serde`/`serde_json` 的实际导入、workspace 继承、新包位置和锁图检查见 [业务依赖教程](../../../modules/example/README.md#使用业务自己的第三方依赖)。

## 消息与命名错误包

`openrust::support::MessageBag` 保存按字段排列的消息，提供 `add`、`merge`、`first`、`get`、`all`、`unique` 和 JSON 编码；纯消息包不依赖 Session。业务代码可直接调用公开 API：

```rust
use openrust::support::{MessageBag, MessageJsonOptions};

fn business_errors() -> openrust::Result<String> {
    let mut errors = MessageBag::new();
    errors.add("email", "邮箱已被使用")?;
    errors.add("title", "请填写标题")?;
    errors.to_json(MessageJsonOptions::UNESCAPED_UNICODE)
}
```

`add` 对数值字符串按 PHP 宽松规则判断重复。例如 `"1e2"` 和 `"100"` 去重，而 `"9223372036854775808"` 和 `"9223372036854775809"` 保留为两条消息。构造去重和 `unique` 使用字符串规则，`merge` 保留重复。底层 `Collection` 保留插入顺序和稀疏键，因此 JSON 不总是连续数组。递归合并可能产生 `MessageValue::Nested`；原始读取与 JSON 保留该结构，无法按上游规则展平的形状通过 `Result` 返回错误。不要用 `all` 替代原始消息读取。

`ViewErrorBag` 按名称保存实现 `MessageBagContract` 的包。`count`、`any` 和 `default_*` 代理只读取 `default` 包；读取缺失包返回临时空包，不隐式插入。业务自定义包通过公开契约接入，已存包的借用与修改保留原实例。

纯错误包调用参考位于[唯一示例模块](../../../modules/example/README.md)的 `POST /api/example/message-bag`。Session 的命名错误包须由应用显式闪存和读取；视图自动共享与验证失败重定向仍属后继范围。

## 请求绑定重定向与错误闪存

业务 handler 可从 `RequestInfo` 创建 `Redirector::from_info(&info)`，调用 `to` 或 `back` 得到默认 302 的 `RedirectResponse`，再用 Axum 的 `IntoResponse` 返回。响应包含 `Location`、HTML 正文和 `text/html; charset=utf-8`；无 Session 的普通重定向也可使用。`back` 按有效 `Referer`、Session 前次 URL、有效 fallback、请求根依次选择目标。目标和状态验证失败返回 `Result` 错误，不把无效 `Location` 写入响应。

需要闪存当前请求输入时，handler 接收公开 `openrust::http::Request`，以 `Redirector::from_request(&request)` 建立响应，再调用 `with_input`、`only_input` 或 `except_input`。这些方法读取同一请求的规范化输入；手动闪存**不会默认排除密码或令牌**，应用应按业务规则显式排除。文件输入未并入此输入仓储；不应把上传文件视为可闪存值。未安装 Session 时闪存返回错误，普通重定向本身仍可返回。

在启用 Session 的应用中，`with_errors(RedirectErrors::Text/Entries/Provider, name)` 将命名错误包闪存到下一请求；下一请求可由 `RequestInfo::session()?.read_errors()?` 读取，`RequestInfo::old_all()?` 读取旧输入。内置 `MessageBag` 无需登记 codec；自定义 `MessageBagContract` 须在 Provider 装配阶段通过 `ErrorBagCodecs::register` 登记稳定标识、版本、编码和解码函数，可用 `RegisterContext::extend::<ErrorBagCodecs, _>` 扩展现有绑定。实现示例见[唯一模块的草稿路由](../../../modules/example/http/controllers/draft_redirect.rs)与[Provider 扩展](../../../modules/example/providers/example.rs)。本请求编码失败、错误格式不合法或 Session 容量超限会返回错误并保留此前状态；自定义 codec 在 Session 状态锁外运行。Rust 的类型化持久格式用于跨请求恢复自定义行为：固定 Laravel Store 的 PHP 序列化保留对象类型，JSON 序列化恢复为基础消息包，此处不承诺 PHP 字节兼容。

本节公开调用链已通过默认/全特性集中门禁、无 Session 与 MySQL8.4 真实 HTTP，以及普通生成应用验证；MySQL 生成认证回归不替代 Session 验收，PostgreSQL 留发布矩阵。`ShareErrorsFromSession`、异常处理器的 `dontFlash` 与自动验证失败重定向仍待后继功能簇。

## 生成用户应用

CLI 保留用户应用生成功能，模板唯一维护于 [CLI](../../../tools/cli/README.md)。例如在临时目录验证：

```bash
cargo run -p cli -- new /tmp/my-application
cd /tmp/my-application
cargo check
cargo test
./artisan list
cargo run
```

生成应用可使用自己的名称、workspace 和 Cargo.lock，本地框架/tool 依赖须保持可达；不修改本仓 workspace，也不要求维护额外源码仓库。标准生成应用默认使用 Redis，运行前准备服务或显式选择其他 cache store。

生成项目的默认二进制按 Cargo 包名构建。例如 `blog` 执行 `cargo build --release --locked` 后得到 `target/release/blog`。发布时将该二进制安装为 `app`，并在同一目录安装可执行的 `artisan`。

未启用数据库、加密或 gRPC 的普通生成项目包含使用 [`TestApp`](../../../vendor/openrust/framework/src/foundation/README.md#测试应用) 的进程内 HTTP 测试；启用数据库认证后另生成隔离数据库的真实 TCP 认证测试与可替换 Mailer 的 `TestApp` 请求。其他生成选项当前不自动生成通用 `TestApp` 测试，应用仍可自行使用该公开 API 并显式提供所需配置。测试以同一 `ApplicationDefinition` Provider 清单启动，不修改全局环境；进程内请求不替代真实服务验收。综合示例还演示业务依赖替换、两应用隔离和移除业务 Provider 后保留健康路由，见 [示例测试](../../../modules/example/tests/registration.rs)。API 默认保留语义 HTTP 状态；只有显式配置 `always_ok` 才将 JSON 响应映射为 HTTP 200。

宿主 JSON API 前缀由 `config/api.rs` 的 `prefix`（部署变量 `API_PREFIX`）选择，配置默认值为 `api`，最终 URI 为 `/api`；挂载和异常范围读取同一配置。模块可以保留自己的 API 路径及响应策略；`/health`、`/ready` 和可选 `/metrics` 保留为运维端点。详细约定见 [HTTP 指南](../../../vendor/openrust/framework/src/http/README.md)。

应用通过已编译的 `config/*.rs` 与显式 `ConfigManifest` 定义配置，在所选 dotenv 与环境快照就绪后求值到同一仓储；不是运行时扫描执行 Rust 文件。`SERVER_PORT=4000 cargo run` 可覆盖内部 `server.port`；`APP_PORT`、独立 `PORT` 和命名空间端口别名不再控制监听。应用可通过 `ApplicationDefinition::config_namespace()` 选择如 `SHOP_` 的应用变量前缀，中性的 `SERVER_PORT` 不随此前缀改变。缓存按 Laravel 的 `CACHE_STORE` 读取 `cache.default`，默认选择已登记的 `redis` store；Redis 连接使用 `REDIS_*`，与队列共用 `REDIS_DB`。其他具名 store 按各自配置连接，不能因名称而假定 driver；自定义 `default` 名称仍合法。框架代码位于独立 `vendor/openrust/framework/` 包，其 `src/` 下包含 audit、config、container、console、foundation、support、core、database、http、translation、mail、sms 等能力模块，以及按 feature 启用的 auth、encryption、signing、queue 和 schedule。应用可编辑配置保存在根 `config/*.rs`；TOML 仅通过显式可选 source 接入，不是默认应用配置。根应用同时承担综合示例与集成验收。

网站静态文件位于应用公开目录，默认是工作目录下的 `public/`。在 `config/server.rs` 配置 `public_dir` 和 `root_target`；可按需选择 `view:index.html`、`file:/index.html`、`forward:/api` 或外部 redirect，视图目标须启用 `view` 并提供实际模板。公开环境覆盖仍经同一配置来源处理；默认欢迎页的完整对应尚未交付，不能以可选视图能力代替该工作流。视图配置与 API 示例见[视图指南](../../../vendor/openrust/framework/src/view/README.md)。
在应用目录复制 `.env.example` 为 `.env`，保留 `APP_KEY=`，然后运行 `./artisan key:generate`。命令把新密钥写入本次启动选定的环境文件。`./artisan key:generate --show` 会把密钥写到标准输出，请只在私有终端运行并避免记录到日志。生产环境已有密钥时会要求确认，自动化应显式传入 `--force` 才执行替换。使用配置缓存时，命令不会重建该缓存。环境文件应只有一条未加引号的原始密钥赋值，写入需要普通文件。进程环境中的 `APP_KEY` 仍优先于文件值，替换后应在部署环境同步更新该值。

应用数据加密通过可选的 `encryption` feature 启用。注册 `openrust::encryption::EncryptionServiceProvider`，在 `[security.encryption]` 下配置 32 字节密钥环，并运行 `openrust key generate` 将独立密钥材料输出到标准输出，再由部署配置将其加入密钥环。该命令不写入 `APP_KEY`。当前密钥用于加密新值；受控轮换期间，历史密钥用于解密旧值。Provider 注册加密服务，首次解析服务时读取并校验密钥环。未使用加密的命令和路由可以运行；首次受保护请求遇到缺失或无效密钥会被拒绝。密钥应保存在版本控制之外。

文件存储由可选 `storage-local` 与 `storage-s3` feature 提供，使用 `FilesystemServiceProvider` 装配 `FilesystemManager` 的 default/named disks。上传与下载都使用有界流，应用须先执行身份和资源授权；本地 disk 仅在当前节点可见。配置、安全边界与 `StorageBackend` 扩展方法见[文件存储指南](../../../vendor/openrust/framework/src/filesystem/README.md)。

可选 `data-exchange` feature 提供有界 CSV/XLSX 逐行解析、显式字段映射与类型转换，以及流式导出；解析器与业务持久化、事务和任务调度分离。CSV 默认防护电子表格公式注入，XLSX 拒绝公式与宏。使用示例和资源边界见[数据交换指南](../../../vendor/openrust/framework/src/data/exchange/README.md)。

`signing` feature 和 `openrust::signing::SignedLinks` 提供绑定用途并会过期的链接。它们使用 `[security.signed_links]` 下独立的密钥环；若需跨实例撤销单条链接，请添加 `SignedLinkRevocationMigration` 并配置共享数据库存储。详见[签名链接指南](../../../vendor/openrust/framework/src/signing/README.md)。

可选 `grpc` feature 使用 tonic 提供 gRPC server/client、具名共享 channel、标准 health、显式 reflection 与 Protobuf 运行时。`GrpcServiceProvider` 注册 `grpc serve` 命令但不会自动监听；服务端默认要求 TLS，明文必须显式设置 `allow_insecure_server = true`，只适用于本机开发或可信 TLS 终止代理。调用预算覆盖流式响应，连接、metadata、消息和全局并发均有上限，停机先报告 `NOT_SERVING` 并限时 drain。reflection 默认关闭，开启时必须显式注册 descriptor set 和服务白名单。具名 HTTPS 客户端绑定显式 CA 与可选客户端身份。CLI 可通过 `openrust new <name> --grpc` 为应用生成独立 proto、build.rs、typed Echo 服务及 Provider；生成编译使用 vendored protoc，不依赖开发机全局安装。生成应用先配置 TLS 证书和私钥，再用 `./artisan serve all` 启动真实 HTTP 与 TLS gRPC 两个监听端口，生成测试通过实际二进制验证 Echo、证书拒绝和信号停机。普通无参数启动仍只运行 HTTP。启用 `auth` feature 后，gRPC server 可验证 JWT/PAT Bearer；方法使用 `GrpcAuth::authorize` 复用 Policy 与 PAT abilities，Cookie Session 不用于 RPC。应用同时安装 `HttpServiceProvider`、`GrpcServiceProvider`、`HttpGrpcServiceProvider` 后可运行 `serve all`；两个 listener 先绑定，仅全部就绪时报告 ready，并共享退出信号和监听停机期限。控制台在监听停止后派发 `Terminating` 并执行应用终止回调。命令作用域和应用资源随后依次清理，各自使用配置的停机时限。嵌入式宿主可用 `runtime::serve_http_and_grpc_with_listeners_until` 交入两个已绑定的 `TcpListener`；先以端口 `0` 绑定并读取实际地址，两个 listener 的所有权会一并交给联合生命周期，准备失败时释放。此入口以传入 listener 的 `local_addr` 决定实际监听范围，`server.bind_addr` 与 `grpc.listen_addr` 不参与绑定或地址匹配校验；TLS 等其他安全与生命周期配置照常生效。普通 `serve all` 继续按配置绑定。该入口不代表多节点路由或集群验收已通过。真实 loopback 四类调用与运行治理见 [`tests/consumers/grpc/`](../../../tests/consumers/grpc/README.md)。

显式审计记录通过 `openrust::audit::Audit` 与数据库 sink 或自定义 `AuditSink` 写入。请将 `DatabaseAuditMigration` 加入应用迁移器；既有数据库另加入 `DatabaseAuditTenantMigration`。默认不接收详情；如需记录，必须配置允许列表，敏感字段名会被脱敏。关键数据库变更可在同一事务中写入经过校验的审计记录，详见[审计模块指南](../../../vendor/openrust/framework/src/audit/README.md)。

`queue` feature 为 PostgreSQL 和 MySQL 提供持久化数据库任务派发、有界 worker 并发和由数据库协调的调度。将 `QueueMigration` 注册到应用迁移器，并在 `DatabaseServiceProvider` 后安装 `QueueServiceProvider`；使用 `register_job` 显式注册处理器。可通过 `schedule_interval`、`schedule_daily_at` 或 `schedule_cron` 注册任务。cron 使用五字段语法，默认时区为 `UTC`，也可指定 IANA 时区；春季不存在的本地时间跳过，秋季重复的本地分钟只派发一次。默认只派发当前分钟匹配项；`MissedRunPolicy::CatchUp { max_slots }` 可设置每次扫描最多补跑 1 到 100 项，后续 tick 继续处理剩余积压。表达式或时区变化会开启新的计划版本，不追溯补发该版本启用前的历史触发。使用集群部署时，各 scheduler 节点必须运行相同构建产物，以保持内置 IANA 时区数据一致。job 派发和时段记录在同一数据库事务提交，两个 scheduler 节点对同一时段只创建一个 job；任务本身仍至少执行一次，外部副作用须保持幂等。`schedule run --once` 扫描一次，`schedule run` 持续运行，`schedule list` 显示已注册项。`Queue::dispatch_unique` 根据队列范围内的键防止任务重叠重复，并返回活动任务 ID；任务完成或移入 `failed_jobs` 后释放该键。`dispatch_chain` 按顺序执行已注册任务，`dispatch_batch` 限制每组任务的并发数。每组是一个持久化父任务；重试可能再次执行已成功子任务的副作用，因此子任务处理器必须保持幂等。每组最多 128 项，批次并发数为 1 到 64。若任务创建需与业务数据共同提交或回滚，请在 `Db` 事务中派发。当前 worker 支持重试、失败任务保留与手动重试、进度、协作取消、单次尝试超时，以及进程故障后的租约恢复。事件支持有序进程内监听器和显式队列监听器；完整集群验收仍待完成。

标准根应用已编译 MySQL、Redis 缓存及 Redis 队列依赖；`openrust new` 默认生成 MySQL 应用，`--db postgres` 选择 PostgreSQL，`--minimal` 保留无数据库/队列的精简应用。库调用方仍可用 `default-features = false` 自行选择能力；`start` 只选择当前应用配置的队列连接。

队列由可编辑 `config/queue.rs` 的 `default` 与 `connections` 唯一配置，默认连接名为 `database`，Redis 连接名为 `redis`。`QUEUE_CONNECTION=redis` 选择 Redis，旧 `QUEUE_DRIVER` 明确报迁移错误；不是第二个连接选择入口。Redis 默认读取同次有效缓存 Redis URL，可用 `QUEUE_REDIS_URL` 独立覆盖，`QUEUE_REDIS_NAMESPACE` 隔离任务键。标准依赖已包含内置 driver，不需为切换连接临时编辑 Cargo 或 Provider。未选连接不创建资源或执行自定义 validator；未知、未编译或无效的具名条目在实际选择时明确失败，不回退数据库、内存或同步执行。当前启动仍检查 `queue.default` 名称及其条目存在，Queue Provider 的完整延迟注册也尚未实现；默认连接错误时点尚未与 Laravel 完全对齐。

先配置所需服务并显式执行 `./artisan migrate`。随后 `cargo run` 消费默认连接的默认队列以及 `config/queue.rs` 的 `embedded_queues`，这些队列共享 `worker_concurrency`。没有默认数据库的应用只有选择 Redis 等可用连接时才启动 worker。使用数据库队列且已登记任务时同进程运行调度器。选用 Redis 队列时，应用会提示已登记的事务型定时任务需要数据库队列。停机时 Web 停止接收新请求，调度器停止扫描新时段，worker 停止领取新任务，并在配置期限内收尾。

分开部署时用 `./artisan serve` 启动 Web，用 `./artisan queue work redis --queue example` 单独消费 Redis 的 `example` 队列。省略连接取 `queue.default`，省略 `--queue` 取该连接的 `queue`（默认 `default`）。Redis 支持普通/延迟/唯一派发、lease、进度、退避重试和失败任务人工重试；数据库事务派发与任务组仍明确不支持。语义为至少一次，handler 必须幂等；Redis 持久性和故障切换取决于部署。详见 [`modules/example/`](../../../modules/example/README.md)。
外部 driver 可通过 `JobRegistry` 复用 typed Job 执行，并用 `JobContext::with_progress_reporter` 声明自己的进度存储；独立的 [`QueueDriver` 示例](../../../tests/consumers/queue-custom-driver/README.md)展示派发、执行和不支持事务时的显式错误。

统一 Provider 装配使用 `QueueServiceProvider` 与容器中的 `QueueContributions`：所有 register 完成后读取最终绑定，任务目录在克隆间共享，后续 boot 注册的任务对已经构造的 driver 可见。`QueueContributions::extend_validated` 将 creator 与最终配置校验器作为同一条目替换；仅 `extend` 时，自定义选项的合法性由实现者负责，框架不会推断第三方配置。最终校验检查管理器全局参数、默认连接名及其存在和已登记任务/调度引用；具名条目保留原始值，只有实际选择时才解码并执行对应 validator。内置 Redis URL/命名空间校验不建立网络连接。worker 停机先封闭整个 manager，所有已构造连接收到停止请求，尚未构造的名称不能在停止后首次初始化；显式重启使用新代缓存，旧代在途结果不能重新发布。


邮件、短信也支持应用实现自定义 driver，并由应用自己的 `ServiceProvider` 注册标准 `Mailer` / `SmsSender` 服务；请看[独立通信 driver consumer](../../../tests/consumers/communication-drivers/README.md)。

```rust
#[cfg(feature = "queue")]
fn main() -> openrust::Result<()> {
use openrust::queue::QueueServiceProvider;
use openrust::schedule::{CronOptions, MissedRunPolicy};

let queue = QueueServiceProvider::new();
queue.contributions().schedule_cron(
    "reports.daily",
    "reports",
    "reports.daily.v1",
    &serde_json::json!({}),
    "30 9 * * 1-5",
    CronOptions {
        time_zone: "Asia/Shanghai".to_owned(),
        missed_runs: MissedRunPolicy::CatchUp { max_slots: 3 },
    },
)?;
Ok(())
}
#[cfg(not(feature = "queue"))]
fn main() {}
```

可选 `realtime` feature 提供在线 WebSocket 与 SSE 会话；跨节点路由须显式启用 `realtime-redis` 并配置 Redis URL，未配置共享 transport 时只支持本进程连接，Redis 故障会返回错误且不会本地静默回退。应用路由负责认证、Origin 校验和频道授权，并为连接提供授权有效期；Session/PAT 撤销需显式调用 `Realtime::revoke_user`。SSE 不提供离线回放，Redis Pub/Sub 不保证远端确认或 exactly-once。连接、消息和缓冲区有配置上限，停机时关闭在线连接。真实双进程示例与验收见 [`tests/consumers/realtime/`](../../../tests/consumers/realtime/README.md)。

## 类型化事件

`EventsServiceProvider` 在同一服务登记监听器。类型所有者实现 `Event::NAME` 声明稳定类名；同名字符串派发使用同一 exact 组。先执行 exact，再按 wildcard 模式首次登记顺序及组内顺序等待监听器；遇到首个错误时立即返回，不再调用后续监听器。该派发仅在当前进程内执行，不会持久化或广播。启用 `queue` feature 后，`listen_queued` 可将事件映射为已注册的 `Job`；在业务事务中调用 `dispatch_in_transaction`，可将任务与业务变更原子地持久化。worker 仍需显式启动，任务处理器必须能够应对至少一次重试。

```rust,ignore
// UserRegistered 由类型所有者声明；这不是框架内建事件。
impl openrust::events::Event for UserRegistered {
    const NAME: &'static str = r"App\Events\UserRegistered";
}
let mut events = openrust::events::EventsServiceProvider::new();
events.listen_queued::<UserRegistered, WelcomeEmailJob, _>("mail", |event| {
    Ok(WelcomeEmailJob { user_id: event.user_id })
});
let mut builder = openrust::foundation::ProviderBuilder::new();
builder.provider(openrust::queue::QueueServiceProvider::new()).provider(events);
let app = openrust::Application::from_provider_runtime(builder.start(config).await?)?;
let events = app.state().resolve::<openrust::events::Events>().await?;

db.transaction(|transaction| {
    let events = std::sync::Arc::clone(&events);
    Box::pin(async move {
        create_user(transaction).await?;
        events
            .dispatch_in_transaction(transaction, &UserRegistered { user_id: 42 })
            .await?;
        Ok(())
    })
})
.await?;
```

同时还需使用 `QueueServiceProvider` 注册 `WelcomeEmailJob`。如需在事务提交后尽力执行进程内观察，请仅在事务成功返回后调用 `Events::dispatch`；监听器错误无法撤销已提交的事务。不得将密码、token、验证码或原始请求放入事件/任务载荷。

## 跨协议校验与执行作用域

`openrust::validation::validate(value)` 把命令、Job 和其他入口的 DTO 校验转换为与 HTTP `Validated<Json<T>>` 相同的字段路径和 `Error::Validation`；校验 trait 与 derive 宏可由 `openrust::validation::Validate` 使用。

`ExecutionContext::current()` 可读取当前 request ID、locale、剩余时间预算和协作取消状态。HTTP 中间件、`Application::run_command` 及 typed Job 执行入口会创建各自的作用域；`JobRegistry::execute` 与数据库 worker 会强制执行 job deadline 和合作取消。直接调用 `JobRegistry::execute` 会在当前异步调用中等待 handler 完成，但不派发、不持久化，也不提供自动重试、事务派发或任务组。存在 `Translator` 服务时，Job 使用派发时的 locale 或应用默认 locale。`tokio::spawn` 不会继承 task-local，需显式传递取消句柄或建立子作用域；子 deadline 不超过父级，子任务超时只取消自身。命令和 Job 可显式调用 `validate` 并在副作用前检查取消/预算。

队列只持久化白名单 `ExecutionMetadata`：最多 128 字节的安全 ASCII request ID 和规范化 locale。身份、token、Cookie、header、deadline 与取消句柄不会进入 payload。自定义 `QueueDriver` 若要接受当前调用中的关联 metadata，须实现 metadata-aware 派发方法；默认实现会在不支持时返回 `queue_driver_metadata_unsupported`。request ID 只用于日志关联，不构成 W3C trace parent 或 exactly-once 保证。

HTTP 的 `/health` 用于存活检查，`/ready` 在 listener 已绑定且未停机时返回 200，停机或未启动时返回 503。收到 SIGINT/SIGTERM 后，服务器先切换为 not-ready，再按 `server.shutdown_timeout_secs` 等待在途请求；到期会合作取消请求并返回 `http_shutdown_timeout`，进程以失败退出。取消不会撤销已提交的副作用。

## v1 源码应用使用路径

生成器创建拥有自己 Cargo.lock 的独立应用 workspace，使用可解析的本地框架和工具依赖；不修改框架源 workspace：

```bash
git clone https://github.com/hongyukeji/openrust.git
cd openrust
cargo run -p cli -- new blog --db postgres
cd blog
./artisan list
cargo run
```

生成的应用应直接依赖仓库内框架源码，通过 `cargo check` 和 `cargo test`，无需发布内部 package 或添加手工 registry patch。依赖编排、自动迁移与 OpenAPI 等保证仍按 v1 计划逐项实现和验收。
## 请求语言

生成的应用默认安装 `TranslationServiceProvider`。`config/i18n.rs` 的 `default_locale` 和 `fallback_locale` 用于选择默认语言和最终资源回退；`Accept-Language` 用于选择受支持的请求语言。框架内置英文和简体中文 HTTP 消息。应用可将 JSON 资源合并至 `MessageCatalog`，在处理器中使用 `RequestLocale`，或调用 `Translator::with_locale` 显式设置任务作用域。普通 `{name}` 文本保持兼容，并支持有限的 CLDR `plural` 类别与 `select` 分支。`Translator::format_number` 接受精确十进制字符串；日期时间格式化接收调用方已转换到明确业务时区的 Chrono 值。详见 [`Translator` API](../../../vendor/openrust/framework/src/translation.rs)。

```rust
use openrust::http::{Inject, Json};
use openrust::translation::{Translator, MessageArgs, RequestLocale};

async fn greeting(locale: RequestLocale, Inject(i18n): Inject<Translator>) -> Json<String> {
    let args = MessageArgs::from([("name".to_owned(), "Ada".to_owned())]);
    Json(locale.translate(&i18n, "app.greeting", &args))
}
```

启动时可将应用 JSON 对象加载至 `MessageCatalog`，例如在指定语言下配置 `{"app.greeting":"Hello, {name}!"}`，或用 `plural` / `select` 对象声明有限分支；两者都要求 `other` 回退项。在请求之外编写邮件或 SMS 时，也应显式使用同一语言设置。数字、日期结果仅用于展示，不应写回 API 机器字段或替换 UTC 存储时间。

事件响应与名称匹配：

- `dispatch`、`dispatch_named` 和对应事务方法返回 `Result<Option<Vec<DataValue>>>`。当前立即派发总是 `Some`；无人监听为 `Some([])`。未来真实延迟机制才可能返回 `None`，当前没有延迟派发能力。
- `listen_reply` 接收类对象并返回 `DataValue`；`listen_named_reply` 接收 `PayloadRef`，一个 `*` 回调可安全下转不同类型。只有轮到要求具体类型的回调才检查载荷；类型不符、类名冲突或非法类声明为服务器配置错误，不作为请求输入错误。
- 普通派发收集 null，遇严格 false 停止且不收该 false。`until`/`until_named` 返回首个非 null，包含 false、0、空数组和字符串“0”；无人响应为 null。`EventPropagation::Stop` 仅截断传播，不自动取消触发事件的业务。错误继续原样传播。
- 类事件与同名字符串共用选择器。只因载荷类型相同不会触发别的类名监听。先 exact，后按 wildcard 模式首次登记次序执行各组；类型登记与同名字符串登记在同一个 exact 组。`EventContext::event_name()` 总是本次完整名称。
- 当前 `DataValue` 数字为有限值、字符串为 UTF-8、对象保留共享身份且不经 JSON 投影。完整 PHP mixed、零/多参数、类接口投影、可变模型视图、广播和延迟/后提交派发仍待对应；不能用普通事件响应冒称完整模型 save 生命周期。

应用可以在现有同一 Provider 登记响应监听，再通过同一个应用 `Events` 读取结果：

```rust,ignore
use openrust::support::DataValue;
events_provider.listen_reply::<UserRegistered, _>(|event, context| {
    Box::pin(async move {
        audit_user(event.user_id, context.transaction()).await?;
        Ok(DataValue::Number(0.into()))
    })
});
let response = events.until(&UserRegistered { user_id: 42 }).await?;
assert!(matches!(response, DataValue::Number(number) if number.as_i64() == Some(0)));
```

只关心成功或错误的业务可以显式丢弃普通响应；不要把普通 false 自动当作认证拒绝或事务回滚。需要业务否决的领域必须使用明确的 `until` 结果解释规则。队列监听器在到达时映射拥有型任务，成功只产生 null；worker 的后续执行结果不能成为当前请求的同步响应。

## 数值日期适配

需要接收 JSON NumericDate 时，使用 `openrust::support::calendar` 的两个独立接口：

```rust
use openrust::support::calendar::{format_php_numeric_timestamp, parse_php_numeric_timestamp};

fn numeric_date() -> openrust::Result<(i64, u32, String)> {
    let instant = parse_php_numeric_timestamp(&serde_json::json!(-1.25))?;
    let formatted = format_php_numeric_timestamp(-1.25)?;
    Ok((instant.epoch_seconds(), instant.microseconds(), formatted))
}
```

返回秒 `-1`、微秒 `250000`、文本 `"-1.250000"`。parser 先将 JSON 数字或合法数值字符串转成 `f64`，交 formatter 生成六位小数，再按 PHP `U.u` 字段解析，使用 UTC，不读取当前时钟或应用时区。字符串首尾只接受空格、TAB、LF、CR、VT、FF 六种 ASCII 空白；不接受 NUL 或 Unicode 空白。bool、null、数组、对象、空串、非法数字及超有符号秒范围会失败，不回退到当前时刻。

formatter 只处理有限 `f64`，不做 JSON 类型转换。六位小数采用 PHP half-up：`0.0000005` 得到 `"0.000001"`，`-0.0000005` 得到 `"-0.000001"`；舍入到零时输出 `"0.000000"`。负数解析按有符号秒与非负微秒分别解释，因此 `-0.25` 得到秒 `0`、微秒 `250000`；它不同于数学负数时间戳或 Carbon timestamp。

大整数也先经浮点阶段，`2^53+1` 可能与 `2^53` 得到同一值，`i64::MAX` 转成浮点后已超过可解析秒范围。JSON 数字不能承载 NaN/Infinity，直接 Rust formatter 遇到这类非有限值会在格式化入口拒绝；这与 PHP mixed 值先格式化、再在 `U.u` 解析失败的阶段不同。此接口不提供完整 PHP mixed、非法 UTF-8 字节字符串或任意对象值域，字符串还受20 MiB资源限制。

唯一[示例模块](../../../modules/example/README.md#数值日期输入)通过现有 `POST /api/example/input` 展示两个可选 `typed.numeric_timestamp`、`typed.numeric_format` 字段。前者接受数字或数值字符串，后者作为业务约束只收 JSON number；未传时响应不增加这两个字段。HTTP 请求仍先经过正常输入中间件，首尾 NUL 裁剪、空串转 null 与直接 Core 调用要分层理解，不能靠关闭归一来模拟 parser。

这两个纯函数不改变 `Request::date` 的默认日期处理，也不实现 Model 日期属性生命周期、JWT Parser 或 ResourceServer。公开外包用法见 [calendar-numeric consumer](../../../tests/consumers/calendar-numeric/README.md)。
