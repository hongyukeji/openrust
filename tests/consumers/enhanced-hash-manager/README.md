# 增强密码管理器公开接口

只用公开 `Hasher` / `HashManager::extend` / `EncodedPasswordHash`，将真正的自定义原Arc登记到同一Manager，验证原驱动身份、成熟Argon实际73字节/72别名、共享调用状态和借用future的Send约束。无数据库或网络服务。

此consumer不改变普通UserProvider，数字码/Flows真实查询与Web/JWT由对应组合测试覆盖。独立验收执行本包的check、严格Clippy及test；不以源码测试定义代替通过记录。
