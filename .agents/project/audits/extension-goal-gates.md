# 持续开发顺序 2–10 的验收状态

顺序 1 由另一工作流维护。本表记录顺序 2–10 已合入的实现及实测范围，外部验收仍以各模块计划为准。

| 顺序 | 本地实现及证据 | 发布条件 |
| --- | --- | --- |
| 2 自定义存储与短信 | `4d45b5609` 提供存储驱动扩展及独立短信消费者，覆盖三语文档与三库／双队列。[待办记录](../todo.md) | 正式环境用计划选定的存储和短信测试身份复测 |
| 3 Webhook | 两个可选包的安装、HTTP 收发、队列／恢复、重放、工厂扩展与运维入口已有 [逐类型验收](webhooks/contracts.json)。[单机与双节点](webhooks/topology.md) 复跑五种本地拓扑，[Worker 时钟偏差](webhooks/topology.md#clock-skew)覆盖共享 PostgreSQL／MySQL，[SQS Standard／FIFO](webhooks/topology.md#sqs-queue)已通过本机 ElasticMQ 实测，[Nginx HTTPS 回调](webhooks/topology.md#nginx-https-ingress)已覆盖证书轮换及节点切换，[目标平台](webhooks/artifact-licenses.md) 核对五组许可依赖图及 ARM64 原生和 amd64 仿真运行镜像，补齐静态 PCRE2、SLJIT 和 AWS-LC 原文 | 真实跨主机、目标队列／对象存储、外部代理和公开 TLS 回调。物理 x86_64 及其他静态依赖和发布渠道保留制品复核 |
| 4 机器翻译 | `07b4ed8d3` 提供七个内置驱动及自定义驱动。[拆分与验收](localization-and-text-translation/README.md)，[LibreTranslate](localization-and-text-translation/README.md#libretranslate-https)已通过隔离 HTTPS、运行时密钥及共享 PostgreSQL Worker 实测 | 真实地区、模型、配额及供应商测试身份 |
| 5 Pay 自定义驱动 | `ea3a39c04` 提供应用级注册。[签名回调](pay/README.md#signed-cluster)已实测三库的 HTTP 和独立 Worker，以及 PostgreSQL／MySQL 双节点回滚与提交后去重 | 隔离商户测试账号、回调及所需验权。真实资金操作需另有授权 |
| 6 Lang Loader | `07b4ed8d3` 提供容器注册、自定义资源、显式刷新及配置缓存。[拆分与验收](localization-and-text-translation/README.md) | 无独立供应商条件 |
| 7 四家内容审核 | `954609cca` 复验十二条供应商协议，点播、关联与复合审核覆盖三库。四家图片和点播通过[私有素材 HTTPS 下载与分段读取](content-moderation/four-provider-recheck.md#four-provider-private-media-https)，签名回调与 Hive 查单提示通过[本机 HTTPS 入口](content-moderation/four-provider-recheck.md#provider-callback-https)。[Sightengine 音轨门禁](content-moderation/README.md#sightengine-audio-track-coverage)已验证未知或多轨无法自动放行 | 四家测试项目、已启用的模型及权限、经批准的语料和预算、供应商可访问的 HTTPS 回调。实际长媒体、多音轨、字幕及效果需以业务样本验证 |
| 8 Reverb 登录 | `01948881c` 验证 Echo、用户登录、定向发送、重连及同主机双节点 Redis。[本地 Nginx HTTPS/WSS](08-02/README.md#nginx-tls-acceptance)及[单节点 Redis 分区](08-02/README.md#redis-user-partition)通过，隔离节点发布返回 503，恢复后继续定向投递 | 发布时在选定反向代理、正式证书和物理节点验证 |
| 9 Cashier | `50988715c` 提供 Stripe／Paddle 具名账户的 HTTP Factory。[含点号的账户](pay/cashier.md#literal-account-name)已验证完整键优先及独立 Redis Worker | 相应测试商户和回调环境 |
| 10 New Relic | `29666971b` 按已确认范围通过 OTLP 模拟及应用生命周期。[验收](../plans/log-supplier-integrations.md#合成业务数据与实际-otlp-模拟验收)，并通过[Windows ARM64 原生 OTLP](../plans/log-supplier-integrations.md#windows-arm64-otlp)和[四区域隔离 HTTPS](../plans/log-supplier-integrations.md#regional-https-otlp)，修正日本区域摄取域名 | 隔离 New Relic 租户、摄取凭据及 NRQL 读权限。实际托管队列和平台按发布环境核对 |

[ARM64 Linux 与 Windows 原生验收](webhooks/native-platforms.md)已验证 Webhook 的 HTTP、Artisan 和 Worker。Linux 运行在本机 Docker。Windows 在虚机用默认 Cargo 测试构建复验，测试入口配置独立的 16 MiB 运行线程。[两个独立虚机](webhooks/topology.md#separate-vm-guests)已通过私有网络投递、共享 PostgreSQL 队列及持久处理终态验收，并实测[共享私有 S3 附件](webhooks/shared-object-storage.md#separate-vm-guests)及[对象 ACL、桶策略和禁用 ACL 的边界](webhooks/shared-object-storage.md#bucket-policy)。物理跨机需已授权的隔离主机及共享服务地址。当前进程未配置内容审核专用凭据目录，也未配置 New Relic 的试验租户、摄取及查询凭据。

扩展组合已核对五个锁定目标的许可依赖图。Linux ARM64 原生容器、AMD64 仿真容器、Windows ARM64／x64 独立 ZIP 和 macOS ARM64 归档各自的 10 个许可文件均逐字节一致，九个包的发现、58 项迁移、缓存前后 HTTP、Reverb 投递和独立 SQLite Worker 启动通过。AMD64 Linux、Windows ARM64／x64 和 macOS ARM64 制品的 New Relic Provider 另通过隔离 OTLP 的 HTTP 指标及 Artisan Span 实测。Windows 两种 ZIP 另验证缓存前后 HTTP 进程响应 Ctrl+C 并以状态 0 退出。Windows x64 在 ARM64 虚机兼容层运行。已通过原始 PE／PDB 核对两种 Windows 制品的原生归档代码与数据，补齐两种架构的 BLAKE3 许可。Linux ARM64、Linux AMD64 与 macOS ARM64 已用实际候选制品的链接映射复核 Cargo 原生归档代码。原生 Linux x86_64 组合部署、原生 Windows x64、Windows Docker Desktop、无 MSVC 工具链的 Windows 安装、macOS Developer ID 签名与公证、完整原生依赖溯源及发布渠道仍需核验。[扩展组合依赖与发行声明](extension-release-licenses.md)
五个候选制品的 [工具链溯源](extension-release-licenses.md#五个候选制品的工具链溯源)已确认 Rust 标准库与编译器内建归档均有保留贡献，并追到 Linux 与 Windows 的启动及运行库来源。Docker 打包和三语独立部署示例已接入 [Rust 标准库版权报告](extension-release-licenses.md#标准库版权报告随附验证)，默认镜像和两份 Windows 九扩展 ZIP 已通过打包及运行复验。后续继续核对 Linux 九扩展完整镜像及目标渠道最终归档。
