# Eloquent：模型基础

当前模型前置提供元数据、原始属性、水合、读取转换、访问器及持久身份。原值/dirty/changes为同一属性存储的状态能力；完整 Eloquent 查询、save、关系、事件、mass assignment、全局作用域与工厂仍在实现；本章不把属性容器等同于完整 ORM。

## 定义模型

`ModelMetadata` 显式声明模型表与类型标识，默认主键 `id`、类型 `int`、自增 `true`。Rust 不反射 PHP 类名，应用在模型构造时选择元数据；同一个实例用于连接选择、水合及持久身份，不从 Guard/Provider 名猜测模型类型。

```rust
use openrust::database::eloquent::{AttributeCast, ModelAttributes, ModelMetadata};
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

扩展只需转换已有原始值时，可使用 `metadata.cast_attribute(name, raw)`；它复用同一读取转换及原始声明检查，不创建属性存储，也不调用 getter/setter 或 SQL。此入口不是完整 `getAttribute`，访问器和缺项策略仍由调用方承担。

只有自增主键才隐式使用 `key_type` 转换；显式 `casts` 优先。UUID 等字符串主键应明确选择非自增与字符串类型。认证默认使用模型主键名及相同属性读取；`GenericUser` 是独立的原始属性账户契约，不偷偷注入模型主键转换。应用选择 `password_hash` 是自身 schema 决定，不更改框架 GenericUser 的默认密码字段 `password`。

## 批量赋值声明

`ModelMetadata::new` 默认白名单为空，拒绝列表为 `["*"]`。应用可用 `fillable` 和 `guard` 替换声明，或用 `merge_fillable` 和 `merge_guarded` 按原顺序合并。示例记录在模型中声明八个可编辑字段，预览按数据库列顺序取交集。批量赋值使用 `attributes.fill(&metadata, input, &policy).await?`，其中 `input` 是保留输入顺序的 JSON 对象，`MassAssignment::new(&databases)` 使用当前应用服务。严格模式可设置 `prevent_silent_discard`，并以 `discarded_attribute_violation` 处理被拒字段。后续字段失败时已执行的赋值保留。示例记录预览先通过账户归属校验和输入验证，再用严格 `fill` 执行模型白名单，metadata 仍由业务 setter 处理。默认策略也可用于独立快照；若字段需要检查 Schema 且未提供数据库服务，将返回 `model_mass_assignment_database_required`。

`force_fill` 在当前异步调用链临时解除保护，复用同一 setter 和 cast，调用结束后恢复保护。另起的 Tokio 任务有独立作用域。启用 hashing 时，可在策略中提供应用 `HashManager`。这两个入口只修改内存属性；模型保存及更完整的自定义转换仍按后续契约交付。

`attributes.is_fillable(&metadata, &databases, "name").await?` 判断白名单、拒绝列表及数据库列；显式白名单优先。拒绝列表非空且没有命中时，已登记 setter 可使虚拟字段参与赋值。其余字段使用模型选择的连接读取 Schema，首次非空结果按模型类标识缓存于当前应用的 `Databases` 服务，空结果保留上游放行语义且会重查。模型类标识需稳定且唯一。自定义 class cast 与 PHP 非默认字节 locale 的匹配仍有对应缺口。
在临时解除赋值保护的调用链中，`is_fillable` 允许赋值，`is_guarded` 仍报告字段的保护规则。

## 水合与读取属性

`attributes.only(&metadata, &["name", "priority"])?` 按指定字段依次应用 getter 和 cast，缺项保留为 `null`。`attributes.except(&metadata, &["name"])?` 先取得当前原始键集合，再排除指定字段并逐项应用同一读取逻辑。数字键按 PHP 宽松比较，空名称和 `"0"` 读取为 `null`。这两个入口接受 Rust 字符串切片，显式属性选择不应用序列化可见性。完整关系和缺项严格模式仍按 Model 后续契约交付。

示例模块的记录预览将选定标题字段放入 `display.summary`，其余可编辑字段放入 `display.details`。例如详情中的 metadata 是经 cast 读取的 JSON 值，现有 `proposed.metadata` 继续展示原始属性文本。预览沿用同一账户归属校验，响应不包含内部所有者标识，也不执行保存。

`attributes_from_row` 从同次查询按列序号读取原始值，不先经过模型 cast 或便利 JSON 转换。SQL NULL、缺失字段与解码失败分开：MySQL 大 unsigned 与 PostgreSQL NUMERIC 保持精确文本；SQLite 按实际 storage class 读取。原始 JSON 列保持文本，只有显式 Array cast 才解码。二进制以字节数组承载；不可表示的非有限数值明确报错，不伪装 null。

精确文本指数据库实际返回的表示；例如 MySQL JSON 存储可能已把超过其整数范围的数字舍入为浮点，属性适配器不会恢复服务器已经丢失的输入数字。数据库允许的时间范围与模型解析器的格式能力也分别验收，不以可解析的 wire 值推断服务器可存储同一个值。

`ModelAttributes::get_attribute` 先调用已登记的 `AttributeGetter`，其余才按元数据读取转换。`register_setter` 登记返回字段补丁的异步访问器；`set_attribute` 在锁外执行转换，成功后合并实际补丁，不能覆盖准备期间的同字段后继更新。非重叠字段更新可保留；读取整份旧快照不意味着对其他字段建立数据库事务。

Array cast 遵循默认 JSON 数值和编码规则：PHP64 大整数按 double 类别读取，合法但无法承载的非有限结果报错；坏 JSON 保持默认 null。JSON 属性载体仍无法表示 PHP 的全部数组键形状、非法 UTF-8 字符串或非有限浮点，这些是保留的对应缺口，不据此宣称完整 cast 已实现。

启用 `hashing` 后，Hashed 赋值使用同一公开 `HashManager`：通过 `set_attribute_with_hashing` 提供应用已解析的服务。null 旁路，明文哈希，已有合法且配置匹配的哈希保持原值；驱动配置错误不覆盖原属性。水合和读取不重新哈希。

## 异步修改与取消

同步短锁只保护原始状态，访问器及异步哈希不跨锁执行。需要在外部操作失败或 future 取消时恢复内存属性，可使用 `set_attribute_reversibly_if` 取得拥有式 `AttributeMutation`：条件不成立返回 None；成功返回前已原子安装整个补丁，确认后调用 commit，否则 Drop 同步恢复仍归本次修改的字段。

多份未决回执沿字段所有权追踪，取消祖先不会复活已取消候选，后继已提交写入和同值 ABA 不被旧回执覆盖。恢复不再次调用用户访问器；用户访问器自行产生的外部副作用及已经发出的数据库 I/O 不在内存回滚保证内。裸 Map 的同步 CAS 闭包须简短且不得重入同一存储（包括自身捕获物析构）；存在未决回执时明确拒绝该不透明批量入口，已知字段补丁与后继正常写入不受此限制。

认证签发 Cookie 读取已确认的原子快照，不使用另一 Guard 尚未持久化的属性值；普通读取仍能观察本次赋值，供正确的 Provider 生命周期使用。详情见[认证](../security/authentication.md)。

## 原始基线与变化记录

对尚未保存的属性调用 `discard_changes()`，会恢复当前原始属性并清空变化记录。
该操作只更新模型内存。仍有未决可回滚写入或正在执行的 setter 时，返回冲突并保留状态。

`get_raw_original` 保留原始值；`get_original` 选择原始字面键或点路径后，使用模型声明的
访问器或转换。缺项默认值也经过转换。整表读取用 `get_original_attributes`，只包含原始
基线已有键。没有声明访问器时传 `|_| None`。访问器读取同一份原始基线的其他字段，
当前实例临时登记的访问器不参与。

`ModelAttributes::new(raw)` 用于水合：输入同时成为当前原值与 original 基线，不执行 setter 或哈希。`Default` 则为空。后续原始或普通赋值不自动覆盖 original；可回滚赋值的 committed 视图表示已确认的内存修改，并不是上次数据库保存的 original。

```rust
use openrust::database::eloquent::{ModelAttributes, ModelMetadata};
use serde_json::{Map, json};

fn main() -> openrust::Result<()> {
    let metadata = ModelMetadata::new("records", "app::Record");
    let attributes = ModelAttributes::new(Map::from_iter([("id".into(), json!("00042"))]));
    attributes.set_raw_attribute("id", json!(42))?;
    assert!(attributes.is_clean(&metadata, &[])?); // 隐式整数cast等价。
    attributes.set_raw_attribute("title", json!("拟修改"))?;
    assert!(attributes.is_dirty(&metadata, &["title"])?);
    attributes.sync_changes(&metadata)?;
    assert!(attributes.was_changed(&["title"])?);
    assert_eq!(attributes.get_raw_original("title")?, None);
    assert!(attributes.get_previous()?.is_empty()); // 新增字段没有旧原值。
    attributes.sync_original_attribute("title")?.sync_original()?;
    assert!(attributes.is_clean(&metadata, &[])?);
    assert!(attributes.was_changed(&["title"])?); // 同步基线不清最近变化。
    Ok(())
}
```

`get_dirty` 只检查当前存在的字段，按原始严格相等、null及相应 cast 规则比较；不调用 getter、setter 或哈希。Array cast 解码后遵循 PHP64 有序数组键与严格数字类型，允许空对象/数组解码等价；原始返回值仍保持自身形状。Float 比较使用四倍 epsilon 内的严格小于判断，内部浮点及 JSON 解码比较可保留非有限值；公开读取输出 `Value` 时仍拒绝非有限数，不能把这一输出限制误用为 dirty 错误。相同原始文本先严格短路，不能据此推断所有 INF/NaN 转换值等价。

`get_raw_original(name)` 先找完整字面键，再按点路径查找，`*` 是字面键；`get_raw_original_attributes()` 返回整份原始映射。缺项为 `None`，显式 null 为 `Some(Value::Null)`。`get_raw_original_or(name, default)` 只在缺项时返回默认值；需要惰性默认时可在读取后使用 `unwrap_or_else`，命中和显式 null 不运行工厂。旧 `raw_attribute` 保持字面字段读取。这里提供原始原值，尚不是会重建模型并运行转换/访问器的完整 `getOriginal`。

`sync_changes` 保存当前 dirty 及这些键与旧 original 的交集 previous，不把属性标 clean。`get_changes`、`get_previous` 和 `was_changed` 读取最近一次发布，空字段列表表示任意变化。`sync_original`、`sync_original_attribute`、`sync_original_attributes` 把当前值确认成内存基线，返回同一实例以便链式调用；不清已有 changes/previous。逐字段同步按输入顺序处理，后项缺失返回 `model_attribute_original_missing`，此前已同步的前缀保留；不要假设它会回滚整批。

未决可回滚写入或仍运行的异步 setter 使比较/同步返回 409 `model_attribute_update_pending`。比较在锁外执行，并以原值及基线版本回验；并发变化返回 409 `model_attribute_snapshot_conflict`，不发布过期 changes。容量耗尽、无效/未支持 cast 和不能输出的转换结果明确报错。主键 `key_type` 的归一类型识别不等于所有入口都忽略原文：例如 `" INT "` 水合后可按整数比较，但普通赋值会在原文 class-cast 检查处失败；`" HaShEd "` 的非空读取会失败；dirty仅在原值不严格相等且本次要对非 null 值执行转换时才进入该拒绝。请使用规范 `int`、`hashed` 或显式 `AttributeCast` 枚举。

这些同步方法只改变内存状态，不执行 SQL、提交事务或模型事件。示例模块的 `POST /api/example/records/{id}/preview` 从同一账户记录构造修改预览，返回 original/proposed/dirty/changes/previous，响应 `persisted=false`；再次读取数据库记录不变。完整保存、事件、并发保存会话、日期/自定义 cast 与完整转换原值查询仍需后续对应。

## 连接与持久身份

`ModelMetadata::resolve_connection` 按模型显式连接、领域 fallback、应用默认连接选择同一 `Databases` 服务；显式空名或 `"0"` 选应用默认，未知名报错。`ModelIdentity::model_key` 由模型属性机制产生，`MorphIdentity` 不把登录 ID 或 Guard 名当作模型类型。完整 morph map、连接生命周期与持久 save 仍须独立对应。

PostgreSQL 二进制时间当前使用 ISO/UTC 表示，不对应任意会话 `TimeZone`/`DateStyle` 的显示格式；这是待闭合的后端语义缺口，不作为必要 Rust 适配关闭。当前 SQLite、MySQL、PostgreSQL 的实现和验收各自取证，不由编译或单后端结果推断全后端完成。

固定来源：[Laravel 13.x Eloquent](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md) 与 [属性访问器和转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md)。完整官方章节仍按依赖逐项交付，当前例子仅说明可用的属性与身份前置。

预览的 original 是现有类型化实体可编辑字段的投影，不是数据库原始行字节水合。JSON比较沿PHP64：超出有符号64位整数范围的数会归为double，可能舍入相等；dirty不能充当任意精度大整数差异检测。预览不改变随后实际更新提交的Value，完整数值/后端对应仍须按范围验收。


需要为可覆盖的模型默认写入行为提供调用级 setter 时，使用 `ModelAttributes::set_attribute_with_default_setter`。同一次快照中已注册的自定义 setter 优先；仅缺省时调用传入的 `&dyn AttributeSetter`，可借用当前服务。两者均在锁外执行，成功补丁进入同一冲突检查；默认 setter 自行决定转换，框架不会随后再运行 cast。准备期间同名 setter 改变会拒绝过期补丁。这个入口不登记长期访问器，也不回滚 setter 的外部副作用。
