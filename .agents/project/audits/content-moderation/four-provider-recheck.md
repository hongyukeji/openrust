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

## 真实验收的前置条件

本轮环境没有配置专用凭据目录。需要以下测试项目及权限，凭据通过受保护目录挂载，勿写入仓库或验收日志。

| 驱动 | 需要的隔离测试资源 |
| --- | --- |
| 阿里云 | 已开通增强文本、图片和点播的地域及审核服务，授权仅限相关送审与查单的测试身份，能使用测试回调校验和短期私有素材链接 |
| 腾讯云 | TMS、IMS、VM 测试账号与三类业务场景、点播查询权限、回调种子及能触发完整分段查单的样本 |
| Sightengine | 测试项目的 API 凭据及文本、图片、点播、OCR、语音模型授权，配置测试项目回调校验。长视频可信分段以已核定的受信素材作为输入，自动生成分段及实际类别／语言覆盖仍待验 |
| Hive | 文本、视觉、OCR、语音测试项目与各自 token，验证 V2 创建的任务能使用获准的查询接口、不同项目数据隔离，以及供应商回调或通知触发受控查单 |

还需测试用途的对外 HTTPS 回调地址与域名授权、四家可访问的隔离私有素材，以及有明确预算的标注样本。业务效果记录按语言、类别、素材、账号及模型区分延迟、漏检、误判、人工复核和实际费用。实际多音轨、字幕及长媒体的覆盖单独验证，协议夹具结果只计本地行为验收。
