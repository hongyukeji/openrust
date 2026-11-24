# 日志供应商原生集成

## LOG-S01：New Relic APM（待选型，未实现）

2026-11-25，用户明确选择“单列供应商集成（推荐）”。固定Logging教程的核心渠道／格式器／处理器按本机实际消费者收尾，本项不阻塞核心章节；不把供应商实现缺失降格为仅缺真实账号或发布验收。

固定文档960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md的formatter:default示例使用Monolog3.10.0 NewRelicHandler，依赖PHP newrelic扩展。当前原生专用Agent及该处理器尚未提供，不能以register_handler或register_formattable_handler回调存在代替完成。

[官方C SDK仓库](https://github.com/newrelic/c-sdk)已于2022年归档，[官方文档](https://docs.newrelic.com/docs/apm/agents/c-sdk/get-started/introduction-c-sdk/)明确停止支持，原目标为64位Linux。不能为照搬PHP示例而默默引入停维SDK，也不能把Log API或OTel的新实现说成原APM调用完全等价。

后续独立选型须核对：维护中的官方接入方式、受支持平台与部署组件、APM事务／错误上下文、结构化Normalizer结果和现有文本Formatter的类型边界、同步／异步时序、凭据与错误恢复、许可和维护成本。先形成具体方案与影响，再由用户确认实施范围。正式实现后按实际类型／Trait／Provider／配置入口，验证正常／拒绝／重试／清理与真实供应商验收；只有本地协议模拟时明确标记边界。

Slack／Papertrail已有原生网络实现和本地真实协议消费者；其真实账号投递与跨平台发布属于原发布矩阵，不与LOG-S01的功能开发缺口混合。历史已闭合功能保持，不推送。
