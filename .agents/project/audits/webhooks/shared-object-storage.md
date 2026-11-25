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
