# `openrust::events` 当前事实

## 当前实现

- 唯一`EventsServiceProvider`登记同Container的`Events`；类事件声明`Event::NAME`，typed与同名string进入同一登记/选择/调用引擎。exact先于wildcard，同组按登记顺序；同载荷类型的不同事件名不互相触发。TypeId首次观察与登记原子绑定，回调在锁外执行。
- `listen_reply`/`listen_named_reply`返回保类型与对象Arc身份的`DataValue`，不经JSON；typed payload只在实际到达监听器时下转，错误保原码。现普通dispatch/具名/事务返回`Result<Option<Vec<DataValue>>>`，立即派发为Some，收集顺序响应；false停止且不加入集合，不自动否决业务。until返回首个非null，false与0也是有效响应，无响应为Null。
- 既有unit listener和queued listener复用同一引擎；传播Stop仅停止本次传播，Err继续由业务调用者传播处理。任务payload仍由显式mapper定义，不因此具备任意PHP mixed持久化。
- `EventContext::transaction()`在事务派发借用实际原始事务；listener/入队失败须由调用者向外传播以回滚。普通分发独立入队，事务内queued在同一事务入队，不等待worker。不可回滚远程副作用应排队，不放提交前direct callback。
- 既有`Db::transaction`提交前事件、提交后direct尽力通知和唯一事务引擎保持；提交后失败不撤销已提交数据，queued不在提交后运行。这里的OpenRust事件身份不代表已对应Laravel官方connection载荷。
- GuardSession事件后仍重新取得许可并核epoch，Fortify官方Web/Session与显式JWT边界保持。事件句柄/队列可能持数据库池，清理须释放真实owner后close测试数据库。
- 普通过程内事件没有持久化、跨节点或exactly-once保证；队列按至少一次执行，handler须幂等。不得向payload写密码、JWT/refresh/PAT、验证码或原始秘密请求/响应。

## 验证与边界

24路径源码树`47b1f9dd62ca7583f53117bef7e60a65bc150553`的日常适用组合验收见[测试基线](../memory/testing.md#事件身份与响应限定验收)。固定PHP源15项参考与Rust/实库分别计证；旧历史验收保存在测试基线与Git。

完整接口投影、多参数、defer/aftercommit、可变Model事件、全部mixed值域与官方事务connection生命周期仍开放；有限身份/响应交付不关闭Events、Model save或原Goal。
