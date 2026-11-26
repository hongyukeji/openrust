# 持续开发顺序 2–10 的验收状态

顺序 1 由另一工作流维护。本表记录顺序 2–10 已合入的实现及实测范围，外部验收仍以各模块计划为准。

| 顺序 | 本地实现及证据 | 发布条件 |
| --- | --- | --- |
| 2 自定义存储与短信 | `4d45b5609` 提供存储驱动扩展及独立短信消费者，覆盖三语文档与三库／双队列。[待办记录](../todo.md) | 正式环境用计划选定的存储和短信测试身份复测 |
| 3 Webhook | 两个可选包的安装、HTTP 收发、队列／恢复、重放、工厂扩展与运维入口已有 [逐类型验收](webhooks/contracts.json)。[单机与双节点](webhooks/topology.md) 复跑五种本地拓扑，[目标平台](webhooks/artifact-licenses.md) 核对五组许可依赖图和原生 ARM64 Release 镜像的声明、许可原文与动态链接项 | 真实跨主机、目标队列／对象存储、外部代理和公开 TLS 回调。其余架构及发布渠道按实际制品复核 |
| 4 机器翻译 | `07b4ed8d3` 提供七个内置驱动及自定义驱动。[拆分与验收](localization-and-text-translation/README.md) | 真实地区、模型、配额及供应商测试身份 |
| 5 Pay 自定义驱动 | `ea3a39c04` 提供应用级注册及三库可信通知消费 | 隔离商户测试账号、回调及所需验权。真实资金操作需另有授权 |
| 6 Lang Loader | `07b4ed8d3` 提供容器注册、自定义资源、显式刷新及配置缓存。[拆分与验收](localization-and-text-translation/README.md) | 无独立供应商条件 |
| 7 四家内容审核 | `954609cca` 复验十二条供应商协议，点播、关联与复合审核覆盖三库。四家项目条件列于[审核复验](content-moderation/four-provider-recheck.md) | 四家测试项目、已启用的模型及权限、经批准的语料和预算、测试 HTTPS 回调。实际长媒体、多音轨、字幕及效果需以业务样本验证 |
| 8 Reverb 登录 | `01948881c` 验证 Echo、用户登录、定向发送、重连及同主机双节点 Redis。[08-02 登录范围](08-02/README.md#user-sign-in) | 发布时在选定反向代理、证书和实际节点验证 |
| 9 Cashier | `50988715c` 提供 Stripe／Paddle 具名账户的 HTTP Factory | 相应测试商户和回调环境 |
| 10 New Relic | `29666971b` 按已确认范围通过 OTLP 模拟及应用生命周期。[验收](../plans/log-supplier-integrations.md#合成业务数据与实际-otlp-模拟验收) | 隔离 New Relic 租户、摄取凭据及 NRQL 读权限。实际托管队列和平台按发布环境核对 |

[ARM64 Linux 与 Windows 原生验收](webhooks/native-platforms.md)已验证 Webhook 的 HTTP、Artisan 和 Worker。Linux 运行在本机 Docker。Windows 在虚机用默认 Cargo 测试构建复验，测试入口配置独立的 16 MiB 运行线程。真实跨机需已授权的隔离主机及共享服务地址。当前进程未配置内容审核专用凭据目录，也未配置 New Relic 的试验租户、摄取及查询凭据。
