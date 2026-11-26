# Webhook 共享 S3 附件

使用既有 `attachment_disk` 配置和框架 S3 适配器。两个应用节点使用同一个私有桶及配置名称，各有独立的应用目录、配置缓存和路由缓存。接收记录保留磁盘名、长度、摘要；Worker 从同一个对象读取原文并校验长度和摘要。接收、暂存补偿及保留期清理复用现在的 Artisan 和数据库队列路径。 [文件系统 S3 基线](../03-03/README.md) · [固定 Laravel S3 兼容文件系统](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/filesystem.md#amazon-s3-compatible-filesystems)

RustFS 1.0.5 真实测试中的 `GetBucketOwnershipControls` 返回 `501 NotImplemented`。已写入对象的 `GetObjectAcl` 返回 `CanonicalUser` 授权但缺少该授权主体的 `ID`，框架拒绝确认私有写入。普通读写结果不能用于绕过 Webhook 的私有可见性检查。改用项目已有 ACL 验收的 SeaweedFS 4.53，测试桶使用 `ObjectWriter`。 [AWS GetObjectAcl](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObjectAcl.html) · [固定 SeaweedFS 4.53](https://github.com/seaweedfs/seaweedfs/blob/4.53/README.md)

## 验收

- 在隔离容器中使用固定 SeaweedFS 4.53 镜像 `sha256:ccad23e18639c8b98cee17e7100b95830a06077b56d8e5b006d6dcf8ec5e94a0`，仅将 S3 端口映射到本机回环地址。验收桶使用随机凭据。经过配置及路由缓存后，由节点 0 接收、节点 1 的 Artisan Worker 重建文件及原始 multipart 正文。匿名 S3 GET 返回 403，数据库原文字段为空。对同一对象改写等长字节后，跨节点读取拒绝损坏数据。
- 断开 S3 连接时，已登记对象的读取失败，两节点的新附件请求均失败且保留原接收和队列状态。连接恢复后，清理失败写入的暂存记录，重试同一请求并从另一节点确认去重、读取原始字节。
- 接收节点在写入对象及事务内入队后退出，待确认的接收和队列记录回滚。另一节点的 Artisan 从已登记的暂存记录删除实际孤立对象。已处理记录到期后先解除对象引用，再由另一节点的 Artisan 删除实际对象。验收使用 SQL 将暂存和保留期记录置为已达到清理时间。
- PostgreSQL 18.7 和 MySQL 8.4.13 各搭配 database 和 Redis 8.10.3，通过四组 S3 及既有双节点用例。原子附件接收仍使用同库 database 队列。SQLite 验证既有本地磁盘路径。使用 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_S3=1` 启用该路径。

验收应用以 `661ba7ff5` 冻结源码和本批夹具构建。应用及两 Webhook 包的格式检查、验收应用严格 Clippy、Python 语法检查通过。快照全工作区格式检查仍报告 [上一批记录的三处既有差异](backend-interruption.md)。旧版 `6d1a81924` 未找到可复用的 Webhook 附件流程。

生产对象存储和真实跨主机仍待 WH-V26／WH-V27。该路径使用可核验的私有 ACL，AWS 默认 `BucketOwnerEnforced`、无 ACL 的服务以及上述 RustFS 版本的 Webhook 私有对象写入未验收通过。 [AWS S3 对象所有权](https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html)

<a id="separate-vm-guests"></a>

## 双虚机共享对象

复用 [Linux 与 Windows 双虚机](topology.md#separate-vm-guests) 的 PostgreSQL 18.7、独立目录和实际 Artisan／Worker，并在同一隔离容器网络启用固定 SeaweedFS 4.53。两端以不同地址连接同一个私有桶。Windows 通过私有网络向 Linux 发送经 HMAC 签名的二进制 multipart，Linux 验签、写私有对象并同库入队。Windows 独立 Worker 走实际 S3 协议重建 multipart 并核对整个原文和文件字节。数据库仅保留附件磁盘名、长度和摘要，匿名 GET 返回 403，双方队列和失败账本为空。

将隔离桶改为 `BucketOwnerEnforced` 后发送另一份正确签名的附件。HTTP 返回 500，数据库没有新增收件和任务，桶中保持原有的一个对象。保留显式私有可见性校验。实验覆盖同一物理宿主的两个虚机和自托管 S3 兼容服务，托管对象存储的策略及权限仍待发布环境验收。证据保存在 `target/tmp/webhook-guest-storage/`，临时数据库、对象、应用目录、容器和网络已清理。

## 对象 ACL 与桶访问策略 {#bucket-policy}

以 `7a6da7d32` 干净源码和 Rust 1.94.0 重建 Linux ARM64 验收应用。复用 PostgreSQL 18.7、SeaweedFS 4.53 固定镜像及现有原子附件接收入口。实际运行 Artisan 迁移、配置和路由缓存，重启后提交经 HMAC 签名的 multipart，经独立 `queue:work --once` 比对存储原文。S3 只映射本机回环端口，桶内全部为合成内容。

| 隔离桶状态 | HTTP、Worker 与 S3 检查 |
| --- | --- |
| `ObjectWriter`，无公开读取策略 | HTTP 200，队列 Worker 逐字节读回原始附件。对象 ACL 只有所有者权限，匿名 S3 GET 返回 403。 |
| `ObjectWriter`，仅测试桶策略允许匿名读取附件前缀 | HTTP 200，Worker 读回原文，读回的对象 ACL 与上一组相同。无签名 GET 返回 200 并取得合成原文。移除该隔离策略后，对同一对象的匿名 GET 恢复 403。 |
| `BucketOwnerEnforced`，无公开读取策略 | 省略 ACL 的对照对象可通过签名请求写入和读回，匿名 GET 返回 403。相同桶里的 Webhook 显式私有附件写入返回 HTTP 500，未新增对象、接收记录或队列任务。 |

Laravel 的可见性选项仍委托底层适配器。内置路径继续使用显式私有 ACL。按 AWS 对象所有权契约，省略 ACL 可兼容 `BucketOwnerEnforced`，但桶和接入点的策略决定实际可访问范围。AWS 的 `GetBucketPolicyStatus` 也采用其定义的公开判定，某些固定 VPC 或端点条件仍可被判为非公开。因此，不能用省略 ACL、读取 ACL 或单一策略状态替代目标环境的私有性核验。禁用 ACL 的内置附件路径当前仍拒绝写入。 [固定 Laravel FilesystemAdapter](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemAdapter.php) · [AWS 对象所有权](https://docs.aws.amazon.com/AmazonS3/latest/userguide/about-object-ownership.html) · [AWS Block Public Access 判定](https://docs.aws.amazon.com/AmazonS3/latest/userguide/access-control-block-public-access.html)

三语指南补充桶策略、接入点和签名／无签名探测要求。静态站点构建、两种部署路径下的三语浏览器检查及冻结源码的工作区格式检查通过。正式验收需隔离桶、可核对所有权与访问策略的部署权限、仅限附件前缀的应用读写删除权限，以及从目标网络发起的无签名测试。托管 AWS 的 IAM、接入点、账号级控制和真实跨机仍待验收。测试桶、容器和网络均已回收，两项服务使用内存文件系统，Docker 数据卷清单一致。并行冷构建的主程序链接被终止，单任务构建通过并重新实测三组。运行及文档日志归档在 `target/tmp/webhook-s3-bucket-policy/`。
