# 06-03 Beanstalkd

按固定 Laravel BeanstalkdQueue、BeanstalkdJob 与连接器核对 tube、默认优先级、延时、TTR、阻塞预约、尝试计数和失败释放。旧版 `6d1a8192` 未找到 Beanstalkd 驱动，沿用既有 Rust JSON 载荷、ReservedJob 和 Worker。 [Laravel BeanstalkdQueue](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/BeanstalkdQueue.php) · [Laravel BeanstalkdJob](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Jobs/BeanstalkdJob.php)

- TCP 会话随预约持有，释放使用原会话。删除前核验预约所有权，旧执行者不能确认被后继会话取得的预约。状态统计提供 ready、delayed 与 reserved 计数。
- macOS ARM64、Rust 1.94.0，使用源码编译的 Beanstalkd v1.13 独立回环实例。真实 `queue:work beanstalkd` 在 SQLite 和 MySQL 8.4.13 失败存储下通过，覆盖延迟、阻塞唤醒、并发预约排他、释放、TTR 到期重投递、过期确认拒绝、失败记录与回调、异常后重试。
- 批次仍限 SQL／Redis。任务链沿用至少一次执行，Beanstalkd 的后继发布与当前任务确认分两步。固定 Laravel Beanstalkd 同样未实现清空接口。SQS 单独验收。
- Rust 格式与专项严格 Clippy、三语站点构建和 12 项浏览器检查通过。
