# 四家内容审核协议复验

在 Rust 1.94.0 下重建启用 `content-moderation,webhook-client` 的测试应用。审核包和独立应用夹具逐文件核对与 `e63e7dcfe` 一致，框架使用隔离累计快照 `d31032e98`。供应商返回由本地 HTTP 夹具提供，驱动、受保护素材、数据库、Artisan 和 Worker 使用实际入口。

| 驱动 | 文本及图片 | 点播提交、查询与处理 |
| --- | --- | --- |
| 阿里云 | SQLite 通过，核对 ACS3、具名语言、私有素材版本和异常 | SQLite、PostgreSQL 18.7、MySQL 8.4.13 通过，含 ACS3、时间证据及状态 |
| 腾讯云 | SQLite 通过，核对 TC3、具名语言、私有素材、OCR 和部分覆盖 | 三库通过，含 TC3、视频时间、覆盖及待处理取消 |
| Sightengine | SQLite 通过，核对规则／ML、multipart、私有版本、OCR 与帧采样 | 三库通过，含查询凭据、取消和时间证据。短视频语音和提前回调关联另在三库复验 |
| Hive | SQLite 通过，核对 V2 项目隔离、UTF-16、multipart、私有版本及采样 | 三库通过，含视觉、OCR、语音和显式查单。未验签通知只触发认证查单，也在三库复验 |

三库的复合审核回归含并发落单、取消与裁决、事务回滚、独立 Worker、修订撤回与清理。SQLite 另复验外部自定义驱动、配置缓存、重复发布、Sightengine 轮换验签、共享调用配额和撤销后的迟到回执。审核包契约、两 feature 的实际夹具及包全目标严格 Clippy 通过。三语文档构建及根路径与 /docs/ 下的六个审核页面浏览器检查通过。执行结果位于 `target/tmp/moderation-four-provider-final/`。

## 阿里云私有素材 HTTPS 入口 {#private-media-https}

在 `c73026bcd` 上用 Rust 1.94.0 构建 Linux ARM64 验收应用。固定 Laravel 的 S3 适配器先签原存储请求，再替换外部地址。沿用现有 OpenRust 适配器、内容审核公开入口和 SeaweedFS 夹具。SeaweedFS 4.53 与 Nginx 1.30.7 使用固定镜像，应用、入口和存储运行在同一宿主机的三个隔离容器。Nginx 用私有 CA 签发的证书对外提供 HTTPS，向存储转发签名时的 Host 和原始路径、查询串，代理访问日志关闭。[固定 Laravel S3 适配器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/AwsS3V3Adapter.php#L82-L98) · [Nginx 代理模块](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass)

真实 HTTP 和独立 Artisan 均收到隔离阿里云夹具的响应。夹具独立验证 ACS3 后，从请求指定的 HTTPS 地址下载素材。分别写入旧版与最新版 PNG、含音轨及仅画面的两段 MP4。四次版本对照逐字节匹配，同一键的旧签名指向旧版本。可信 CA 下，无签名请求、替换为另一个已存在的版本 ID、修改签名均返回 403。使用系统 CA 时拒绝握手。生成配置与路由缓存后，继续校验已缓存的送审行为、审核结果和日志脱敏。SQLite 上独立的 Artisan Worker 完成点播查单，队列和失败任务表均为空。

预置测试素材时，容器内 curl 7.88.1 的带正文请求签名首轮被存储拒绝。共享测试夹具在签名前加入正文的 `x-amz-content-sha256`，版本管理操作使用显式空查询值，随后图片与点播全流程通过。MP4 在宿主机实际生成并由 ffprobe 核对音轨后，只读挂载至验收进程。日志保存在 `target/tmp/moderation-media-https/`。

Rust 1.94.0 全工作区格式检查与 macOS ARM64 的验收应用严格 Clippy 通过。Python 语法、三语站点构建、根路径及 `/docs/` 下的浏览器检查和 S3 文档链接锚点通过。Linux Clippy 运行期间 Docker 控制接口一度超时，编译进程以 SIGKILL 结束。同一源码由宿主机复核。Docker 恢复后，用旧 Webhook 本机存储入口验证了签名写入、读取、匿名 403 与删除。隔离容器、网络、证书和临时源码均已清理。

复跑 `tests/moderation_aliyun_image_acceptance.py` 和 `tests/moderation_aliyun_video_acceptance.py` 时，用 `OPENRUST_TEST_MODERATION_MEDIA_CA` 传入私有 CA 文件，并传入隔离 S3 的 `AWS_ENDPOINT`、`AWS_BUCKET`、`AWS_ACCESS_KEY_ID` 和 `AWS_SECRET_ACCESS_KEY`。代理的证书和 DNS 名须匹配夹具的 `moderation-media.fixture.invalid`。点播夹具默认用 FFmpeg 生成素材，也可通过 `OPENRUST_TEST_MODERATION_VIDEO_DIR` 提供已生成并核验音轨的两个样本。

本次覆盖阿里云图片与点播使用 HTTPS 读取私有对象的隔离路径。其他三家的协议测试范围见上表。正式 DNS、公开 CA、厂商出站网络和托管 S3 权限随目标环境验证。

## 四家私有素材 HTTPS 与分段读取 {#four-provider-private-media-https}

在 `4da97d589` 快照上，用 Rust 1.94.0 重建 macOS ARM64 验收应用。沿用固定 Laravel S3 签名边界和上一批的版本化素材契约。四家图片、点播共用同一个下载探针，配置缓存、各供应商签名与结果断言沿用各自的夹具。

本机编译 Nginx 1.30.7，使用校验过 SHA-256 的 SeaweedFS 4.53 ARM64 官方制品。两者监听隔离的回环地址。访问端仅在验收进程内将 `moderation-media.fixture.invalid` 映射到 Nginx 的动态端口，请求保持原 SNI，并用专用 CA 验证证书。Nginx 转发签名时使用的源站 Host、路径及查询串。访问日志关闭。视频样本用 FFmpeg 生成，并经 ffprobe 区分有音轨和无音轨的版本。[Nginx 源码](https://nginx.org/download/nginx-1.30.7.tar.gz) · [SeaweedFS 4.53](https://github.com/seaweedfs/seaweedfs/releases/tag/4.53)

阿里云、腾讯云、Sightengine、Hive 的八组真实 HTTP 和 Artisan 用例通过。每组对比旧版、新版素材的字节，使用同一签名链接读取 `bytes=0-15` 得到 206 和正确的 16 字节。系统 CA 拒绝私有测试证书。无签名、换成另一有效对象版本、修改签名均返回 403。四家点播用例另经独立 SQLite 队列 Worker 查单。Sightengine 与 Hive 的图片上传、OCR 和部分覆盖断言继续通过，腾讯云点播保留取消断言。

全工作区格式检查、macOS ARM64 的应用目标严格 Clippy、Python 夹具加载、三语站点构建与两种挂载路径的浏览器检查通过。Linux 冷构建在 `jixel` 代码生成阶段以 SIGKILL 结束，八组结果来自原生 macOS 实跑。确认 Linux 容器和原生服务退出后，已回收隔离源码、证书、样本和文档产物。执行日志位于 `target/tmp/moderation-provider-media-https/`。本次使用厂商协议夹具和私有测试 CA。正式访问权限、域名、公信证书及实际厂商出站下载继续在目标环境验收。

## 本机 HTTPS 回调入口复验 {#provider-callback-https}

在 `894955da7` 快照上，用 Rust 1.94.0 构建 macOS ARM64 验收应用，同时启用 `content-moderation` 和 `webhook-client`。复用原有供应商协议、固定素材和回调消费规则。Nginx 1.30.7 使用私有 CA 签发的 `callback.example.test` 证书，只绑定回环地址。客户端保留原 SNI。信任测试 CA 时可连接，系统 CA 拒绝握手。代理保持原请求体与签名头，访问日志不记录载荷。素材由同机独立 SeaweedFS 4.53 提供。[Sightengine 验签说明](https://sightengine.com/docs/callback-signature) · [腾讯云回调配置](https://cloud.tencent.com/document/product/1265/104001) · [Hive 异步回调说明](https://docs.thehive.ai/reference/submit-a-task-asynchronously)

- 阿里云、腾讯云和 Sightengine 经 HTTPS 接收提前回调。独立 Artisan 数据库 Worker 先延期，收到原任务 ID 后再关联并经认证查单。在相关用例中，正文被改动、缺失或重复签名、错误 UID 与内容类型均按各供应商契约拒绝，重复投递不触发额外查单。
- Hive 提交时登记的 URL 与实测 HTTPS 路由一致。该入口接受匹配任务的无签名提示，拒绝伪造签名和超大正文，并实测去重、限流及租户隔离。只有独立 Worker 的认证查单推进审核，数据库中无已验签回调记录。
- Sightengine 另经 HTTPS 验收终态后的迟到通知与撤回后墓碑清理。最终六组 HTTPS 用例和阿里云直连、Hive 直连、Hive 点播三组回归通过，使用独立 SQLite 数据库队列及实际 HTTP、Artisan 和 Worker。

全工作区格式检查、应用严格 Clippy、Python 加载与三语站点构建通过。内容审核页面在根路径与 `/docs/` 下的六项浏览器检查通过。全站浏览器检查在快照既有的英语 AI SDK 页面因缺少原生参考链接失败，该页面没有改动。日志位于 `target/tmp/moderation-provider-callback-https/`。正式域名、公信证书及供应商出站回调仍待授权环境验证。

## 真实验收的前置条件

本轮环境没有配置专用凭据目录。需要以下测试项目及权限，凭据通过受保护目录挂载，勿写入仓库或验收日志。

| 驱动 | 需要的隔离测试资源 |
| --- | --- |
| 阿里云 | 已开通增强文本、图片和点播的地域及审核服务，授权仅限相关送审与查单的测试身份，能使用测试回调校验和短期私有素材链接 |
| 腾讯云 | TMS、IMS、VM 测试账号与三类业务场景、点播查询权限、回调种子及能触发完整分段查单的样本 |
| Sightengine | 测试项目的 API 凭据及文本、图片、点播、OCR、语音模型授权，配置测试项目回调校验。长视频可信分段以已核定的受信素材作为输入，自动生成分段及实际类别／语言覆盖仍待验 |
| Hive | 文本、视觉、OCR、语音测试项目与各自 token，验证 V2 创建的任务能使用获准的查询接口、不同项目数据隔离，以及供应商回调或通知触发受控查单 |

还需测试用途的对外 HTTPS 回调地址与域名授权、四家可访问的隔离私有素材，以及有明确预算的标注样本。业务效果记录按语言、类别、素材、账号及模型区分延迟、漏检、误判、人工复核和实际费用。实际多音轨、字幕及长媒体的覆盖单独验证，协议夹具结果只计本地行为验收。
