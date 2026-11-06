# SQLx MySQL 冷预处理取消补丁

来源：crates.io `sqlx-mysql` 0.9.0，源码 commit `003b698e99e024f3621b8043a2426fde5b741171`。原 MIT / Apache-2.0 许可及上游文件保留；本地修改包括下述取消保护和只读列元数据通路。原始逐文件 SHA256 位于本文件同目录的 `UPSTREAM-SHA256.json`，该清单始终表示上游原文件，不冒充本地修改后的散列。

SeaORM 2.0.4 原生对照可复现：冷 PREPARE 首次等待时取消，后续响应错帧且 rollback 可挂起；预热或实际服务器行锁等待后取消不触发同一缺陷。本补丁不重建未知协议流：发送前标记 prepare，完整响应或已完整解码的数据库 ERR 后才恢复。遗留状态拒绝后续请求和显式提交/回滚；归还池时淘汰单条物理连接，不冲刷未知缓冲、不发送 QUIT/ROLLBACK。Result 只在真实执行命令完整排入缓冲后登记。

取消本身不保证服务端已回滚。调用方继续持有废事务仍可能持锁，归还/销毁句柄、物理连接断开后由服务器回滚。正常数据库错误保留复用；预热及服务器已确认行锁等待取消保留既有恢复路径。完整网络协议任意阶段取消不是本补丁已证明的范围。

Cargo 只应用最终 workspace 根的 patch：根源码与 CLI 生成应用自动登记，手写消费者须在自己 workspace 根设置相同 `sqlx-mysql` path 补丁。暂精确固定 SQLx 0.9.0，避免新版本精确叶依赖绕过补丁；发布或升级前须核上游修复、真实解析图和适用实服务回归。不是发布到 crates.io 的替代发行。

## 原始模型属性的只读元数据

`type_info.rs` 公开原始协议类型编号、unsigned/ZEROFILL标志和可选column_length；`value.rs` 公开本次值的text/binary格式。`protocol/text/column.rs` 已解析的decimals经 `connection/executor.rs` 保留到 `column.rs`，以 `fractional_digits()` 返回；旧offline数据缺失时保持None，不伪造零位精度。

这些getter不发送SQL、不改值解码、不依赖Debug格式，不改变现有TypeInfo.name兼容逻辑。原因是名称会把TINYINT(1)合并为BOOLEAN，且不包含ZEROFILL/小数秒；模型raw水合必须区分PDO的原始数值/文本类别与后置属性cast，不能先被便利JSON解码损失精度或格式。当前候选随Model属性簇集中验收，先前取消保护的旧证据不证明此新增通路已通过。
