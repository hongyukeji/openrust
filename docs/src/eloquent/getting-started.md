# Eloquent：模型基础

当前模型前置提供元数据、原始属性、水合、读取转换、访问器及持久身份。完整 Eloquent 查询、save/dirty、关系、事件、mass assignment、全局作用域与工厂仍在实现；本章不把属性容器等同于完整 ORM。

## 定义模型

`ModelMetadata` 显式声明模型表与类型标识，默认主键 `id`、类型 `int`、自增 `true`。Rust 不反射 PHP 类名，应用在模型构造时选择元数据；同一个实例用于连接选择、水合及持久身份，不从 Guard/Provider 名猜测模型类型。

```rust
use openrust::db::eloquent::{AttributeCast, ModelAttributes, ModelMetadata};
use serde_json::{Map, json};

fn main() -> openrust::Result<()> {
let mut metadata = ModelMetadata::new("users", "app::models::user::User");
let attributes = ModelAttributes::new(Map::from_iter([
    ("id".into(), json!("00042")),
]));
assert_eq!(attributes.get_attribute(&metadata, "id")?, Some(json!(42)));
assert_eq!(attributes.raw_attribute("id")?, Some(json!("00042")));

metadata.incrementing = false;
assert_eq!(attributes.get_attribute(&metadata, "id")?, Some(json!("00042")));
metadata.casts.insert("id".into(), AttributeCast::String);
assert_eq!(attributes.get_attribute(&metadata, "id")?, Some(json!("00042")));
Ok(())
}
```

只有自增主键才隐式使用 `key_type` 转换；显式 `casts` 优先。UUID 等字符串主键应明确选择非自增与字符串类型。认证默认使用模型主键名及相同属性读取；`GenericUser` 是独立的原始属性账户契约，不偷偷注入模型主键转换。应用选择 `password_hash` 是自身 schema 决定，不更改框架 GenericUser 的默认密码字段 `password`。

## 水合与读取属性

`attributes_from_row` 从同次查询按列序号读取原始值，不先经过模型 cast 或便利 JSON 转换。SQL NULL、缺失字段与解码失败分开：MySQL 大 unsigned 与 PostgreSQL NUMERIC 保持精确文本；SQLite 按实际 storage class 读取。原始 JSON 列保持文本，只有显式 Array cast 才解码。二进制以字节数组承载；不可表示的非有限数值明确报错，不伪装 null。

精确文本指数据库实际返回的表示；例如 MySQL JSON 存储可能已把超过其整数范围的数字舍入为浮点，属性适配器不会恢复服务器已经丢失的输入数字。数据库允许的时间范围与模型解析器的格式能力也分别验收，不以可解析的 wire 值推断服务器可存储同一个值。

`ModelAttributes::get_attribute` 先调用已登记的 `AttributeGetter`，其余才按元数据读取转换。`register_setter` 登记返回字段补丁的异步访问器；`set_attribute` 在锁外执行转换，成功后合并实际补丁，不能覆盖准备期间的同字段后继更新。非重叠字段更新可保留；读取整份旧快照不意味着对其他字段建立数据库事务。

Array cast 遵循默认 JSON 数值和编码规则：PHP64 大整数按 double 类别读取，合法但无法承载的非有限结果报错；坏 JSON 保持默认 null。JSON 属性载体仍无法表示 PHP 的全部数组键形状、非法 UTF-8 字符串或非有限浮点，这些是保留的对应缺口，不据此宣称完整 cast 已实现。

启用 `hashing` 后，Hashed 赋值使用同一公开 `HashManager`：通过 `set_attribute_with_hashing` 提供应用已解析的服务。null 旁路，明文哈希，已有合法且配置匹配的哈希保持原值；驱动配置错误不覆盖原属性。水合和读取不重新哈希。

## 异步修改与取消

同步短锁只保护原始状态，访问器及异步哈希不跨锁执行。需要在外部操作失败或 future 取消时恢复内存属性，可使用 `set_attribute_reversibly_if` 取得拥有式 `AttributeMutation`：条件不成立返回 None；成功返回前已原子安装整个补丁，确认后调用 commit，否则 Drop 同步恢复仍归本次修改的字段。

多份未决回执沿字段所有权追踪，取消祖先不会复活已取消候选，后继已提交写入和同值 ABA 不被旧回执覆盖。恢复不再次调用用户访问器；用户访问器自行产生的外部副作用及已经发出的数据库 I/O 不在内存回滚保证内。裸 Map 的同步 CAS 闭包须简短且不得重入同一存储（包括自身捕获物析构）；存在未决回执时明确拒绝该不透明批量入口，已知字段补丁与后继正常写入不受此限制。

认证签发 Cookie 读取已确认的原子快照，不使用另一 Guard 尚未持久化的属性值；普通读取仍能观察本次赋值，供正确的 Provider 生命周期使用。详情见[认证](../security/authentication.md)。

## 连接与持久身份

`ModelMetadata::resolve_connection` 按模型显式连接、领域 fallback、应用默认连接选择同一 `Databases` 服务；显式空名或 `"0"` 选应用默认，未知名报错。`ModelIdentity::model_key` 由模型属性机制产生，`MorphIdentity` 不把登录 ID 或 Guard 名当作模型类型。完整 morph map、连接生命周期与持久 save 仍须独立对应。

PostgreSQL 二进制时间当前使用 ISO/UTC 表示，不对应任意会话 `TimeZone`/`DateStyle` 的显示格式；这是待闭合的后端语义缺口，不作为必要 Rust 适配关闭。当前 SQLite、MySQL、PostgreSQL 的实现和验收各自取证，不由编译或单后端结果推断全后端完成。

固定来源：[Laravel 13.x Eloquent](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md) 与 [属性访问器和转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md)。完整官方章节仍按依赖逐项交付，当前例子仅说明可用的属性与身份前置。
