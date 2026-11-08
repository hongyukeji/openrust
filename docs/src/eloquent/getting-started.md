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

## 日期属性

在 `ModelMetadata::casts` 中声明 `AttributeCast::DateTime`、`Date` 或其不可变变体。使用 `get_attribute_value` 读取日期对象，使用 `snapshot` 读取其时刻、时区与微秒。`Date` 将时间调整到本地午夜，`Timestamp` 返回 Unix 秒。带格式的 `CustomDateTime` 和 `ImmutableCustomDateTime` 保留时间，并在调用 `project_attribute_json` 时按给定 PHP 格式输出。普通日期的 JSON 投影使用 UTC 和六位微秒。

使用 `with_date_context(ModelDateContext::for_connection(calendar, &db)?)` 将应用 Calendar 和已选连接的查询语法交给模型。独立模型可用 `ModelDateContext::new` 显式指定语法格式。应用可在 `CalendarContext::with_date_factory` 安装构造回调，替换或调用 `DateConstruction::default_date` 装饰默认结果。HTTP Provider 在 `boot` 中通过 `BootContext::extension_mut::<CalendarContext>()` 登记包含应用时区的上下文。

显式 `date_format` 优先用于普通日期赋值。自定义 cast 的格式用于输出，单独声明它时原始赋值保持原样。日期对象走 `set_attribute_value`，普通 JSON 输入仍走 `set_attribute`。

自动时间戳从应用工厂的 `DateConstruction::Now` 取一次时刻。新模型的创建和更新时间共享该时刻，已经手工改动的列保持原值。可用 `ModelDateContext::with_serializer` 覆写普通日期和有类型 getter 返回日期的输出，调用 `ModelAttributes::project_attribute_json` 使用该规则。带自定义格式的 cast 仍按其格式投影，写库仍使用模型存储格式或连接的绑定格式。

整组属性可用 `ModelAttributes::attributes_to_array` 投影。它先按 `visible` 和 `hidden` 选列，再处理日期名单、访问器和 cast。日期名单的输出传给访问器作当前值，访问器的原始快照仍保留存储值。使用 `attributes_to_json` 可继续转换尚未投影的原始日期对象。

`AttributeGetter` 和 `AttributeSetter` 接收 `AttributeValue` 与 `AttributeMap`，可以从同一快照读取 JSON 值和日期对象。只处理 JSON 的访问器可对实际使用的字段调用 `as_json`。旧式 getter 返回的日期在属性数组中保留对象身份，进入 JSON 时采用日期对象的默认输出；标记访问器返回的日期仍使用模型序列化覆写。

用 `ModelMetadata::append`、`merge_appends` 或 `set_appends` 声明数组输出需要的计算字段，并登记对应的读取访问器。追加字段在日期、访问器和 cast 处理后求值，使用原始属性，遵守 `visible` 与 `hidden`。未登记访问器的可见追加字段返回 `model_appended_accessor_missing`。示例记录预览用这一机制输出 `is_published`，原始属性和数据库列保持独立。

### 序列化模型与已加载关联

实现 `HasRelationships` 的模型通过 `relations_to_array` 单独投影已加载关系，通过 `to_array` 返回有序模型数组，通过 `json_serialize` 返回 JSON 值，或通过 `to_json` 返回 JSON 文本。先转换属性与计算追加字段，再合并已加载的关系。关系可以覆盖同名属性。单值关系加载后无匹配时为 `null`，空集合为 `[]`。序列化不查询未加载关系。

`visible` 与 `hidden` 使用加载时的关系名称。默认输出蛇形键名，可将 `ModelMetadata::relation_naming` 设为 `RelationNaming::Original`。子模型各自应用属性、可见性和追加字段规则。多对多模型包含已加载的中间表模型。同一实例在当前递归路径内重复出现时只输出其属性。

手动安装关系可用 `ModelRelations::set_model` 或 `set_collection`，保留已加载模型的类型和实例身份。通过 `set_relation` 存放的其他类型擦除值仍可下转，模型序列化略过这类值。

### 自定义类转换

在 `ModelMetadata::casts` 使用 `AttributeCast::Class(ClassCast::new(factory))`。工厂返回实现 `ClassCaster` 的转换器，可捕获构造参数。转换器的 `get` 读取模型原始快照，默认透传原值。仅入站转换使用 `ClassCast::inbound(factory)`，对象赋值也不会进入缓存。用 `ClassCastWrite::Value` 写入当前字段，用 `ClassCastWrite::Patch` 一次提交多列补丁。正常读取和赋值保留已有访问器优先级。

用 `with_serialization` 启用 `serialize` 以定义非 null 的数组输出。用 `with_comparison` 启用 `compare` 以比较持久化基线与当前原值。数组化保留 Laravel 的分派顺序，带访问器或追加声明的类转换直接使用读取结果。示例记录的 `view_count` 通过公开类转换完成写入、回读与预览。覆写 `get_value` 可返回 `ModelObject`。同一模型重复读取共享对象句柄，`modify` 更新有类型快照，保存前由转换器的 `set` 回写原始列。用 `without_object_caching` 关闭对象缓存。模型数组保留对象身份，JSON 使用对象的显式投影或 `serialize_value`。

### 数组对象与集合转换

`AsArrayObject::cast()` 把已存 JSON 数组读取为 `ArrayObject`。通过 `ModelObject::modify` 修改对象时，可用 `as_collection_mut` 更新底层键值，保存前会将同一对象回写到原始列。读取和序列化保留关联键和稀疏数字键。无效 JSON、标量及缺项读取为 null。

`AsCollection::default().cast()` 使用标准 `Collection`。`AsCollection::using(factory)` 可提供实现 `CollectionCastValue` 的类型化集合工厂，`AsCollection::of(map)` 接收值与键并保留原键。`of_objects` 保留元素类型，并分别应用其模型数组和持久 JSON 投影。工厂或映射以 Rust 函数表示，替代 PHP 的运行时类名。示例记录预览用这两个 cast 计算元数据标签数量。默认赋值 null 写 JSON 文本 `null`，`AsArrayObject::nullable()` 与集合声明的 `nullable()` 改写 SQL NULL。

### 原生加密属性

启用 `encryption`，从应用状态解析 `dyn ApplicationEncrypter`，然后在模型的
`metadata.casts` 声明 `AttributeCast::Encrypted(EncryptedCast::new(kind, encrypter))`。
`EncryptedCastKind::String` 对应 Laravel 的 `encrypted`，其余
`Array`、`Json`、`Object`、`Collection` 分别对应四种 `encrypted:…` JSON 转换。
`Collection` 通过 `get_attribute_value` 返回独立集合，模型序列化会输出集合内容。

建议使用 TEXT 列保存密文，空值仍存为 SQL NULL。读取可用历史密钥，赋值总用当前密钥。
有历史密钥时，重新加密产生的不同密文视为已变更。无历史密钥时，标量比较明文，
JSON 变体比较解码后的有序关联数组。Rust 标量字符串要求明文为有效 UTF-8。
使用 `set_attribute(&metadata, "private_metadata->preferences->locale", json!("zh-CN"))`
更新嵌套值。路径也接受点分隔，数字键按 PHP 有序数组处理。底层列使用原生或内置类加密
转换时，模型先解密整个 JSON，再以当前密钥保存更新后的值。赋值成功后会清除底层列的对象缓存。
使用批量赋值时，应将完整路径列入 `fillable`。

唯一示例的 `/api/example/records/{id}/private-metadata` 使用原生 JSON 加密转换读写专用列。
`PATCH` 接受 `path` 和 `value`，在事务中锁定当前账户的记录后更新。读取响应禁用缓存。
字符串根支持数字字节偏移，结果须是有效 UTF-8。为避免小输入产生大分配，单次偏移最多补齐 1 MiB。

### 加密数组对象与集合

启用 `encryption` 后从应用状态解析 `dyn ApplicationEncrypter`，交给 `AsEncryptedArrayObject::cast` 或 `AsEncryptedCollection::cast`。用 `AsEncryptedCollection::using` 可复用 `AsCollection` 的类型化工厂与映射。两者用 Laravel 字符串载荷保存 JSON，null 保持 SQL NULL。已加载对象按现有缓存规则回写。

```rust,ignore
use openrust::{
    database::eloquent::AsEncryptedCollection,
    encryption::ApplicationEncrypter,
};

let encrypter = app.state().resolve::<dyn ApplicationEncrypter>().await?;
metadata.casts.insert(
    "private_notes".into(),
    AsEncryptedCollection::cast(encrypter),
);
```

加密数组对象要求解密后的 JSON 为数组。加密集合将解码 null 转为空集合，标量转为单元素集合。认证或解密错误会返回错误。无历史密钥时，原值比较使用解密后的原始字符串；配置历史密钥后，原始密文发生变化就会保持 dirty，保存使用当前密钥。

### 有类型的访问器

用 `ModelAttributes::register_attribute` 登记 `ModelAttribute`。它的同步 getter 接收原始属性值和快照，setter 返回有序的多字段补丁。日期对象默认按字段缓存。再次读取该字段时会调用配置的 setter，同步对象的当前值。读取全部原值、同步原始基线或计算 dirty 时会同步全部适用的缓存。

调用 `without_object_caching` 可让 getter 每次产生新值，`should_cache` 可同时缓存普通值。访问器在属性锁外运行，回写提交前会检查快照和日期对象的版本。替换原始快照会清除对象缓存。

保存期间已绑定到 SQL 的可变日期可以读取。此时修改会返回 `model_date_binding_pending`，查询结束或取消后可再次修改。模型的更新回执使用本次实际绑定的列。

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

## 保存模型

`save` 通过 `SaveModel` 读取模型是否已持久化及实际列绑定。已有记录按原始主键更新，未变更时仍触发 saving 和 saved。新记录先生成启用的唯一 ID，再派发 creating，随后处理时间戳和插入。自增 ID 来自同一次 INSERT，并经模型 setter 赋值。成功插入后，模型先更新 exists 和 recently-created，再派发 created。

模型为写库与变化发布提供属性存储的独占借用。保存结束时先派发 `saved`，若模型仍有变化且 `SaveContext::touch` 启用，则执行模型的 `touch_owners`，之后同步原始基线。关联类型的自动遍历与数据库表达式回读仍在后续范围。插入已成功但主键赋值或后置监听失败时，需核对实际存储状态再重试。

`save_quietly` 复用正常保存流程，在当前异步调用链内停止模型事件，其他应用事件仍可派发。`update` 只对已有模型执行普通批量赋值并保存。`update_or_fail` 在赋值后使用事务保存，`update_quietly` 则在调用链中屏蔽模型事件。`force_create` 由应用工厂构建新模型，临时解除批量赋值保护直至保存返回；若创建事件取消，可通过模型的 exists 判断结果。Builder 的 pending attributes 装配仍待接入。

`save_or_fail` 使用所选模型连接的现有数据库事务执行同一个 `save`。发生错误时由事务执行器回滚 SQL；事件取消仍返回 `false`。同一查询携带的事务会传给模型监听器及关联 touch 回调，监听器可用它读取未提交的修改。模型内存状态和事务之外的监听器副作用遵循普通保存语义。

`refresh(&mut model, &context)` 从模型主库按原主键回读。模型通过 `HydrateModel` 提供水合构造，框架在独立实例上派发 `retrieved`，再安装该实例的当前原始属性，重载原模型已经加载的非 Pivot 关系，最后同步基线。关系重载失败时保留新属性和旧基线。最近一次保存的变化记录保持。新模型直接返回。`refresh_for_update` 需要模型连接签发的写事务，并持锁直到事务结束。MySQL 和 PostgreSQL 使用行锁，SQLite 复用既有的立即写事务。

`fresh` 使用同一水合与派发流程，直接返回独立实例，保留原模型的内存修改。新模型或缺行时返回空结果。`fresh_with` 在水合事件之后按指定名称预加载关系。模型通过 `HydrateModel::load_relation` 将名称接到关系声明。锁定回读把事务上下文传给关系加载器。使用 `HasMany::load_in_transaction` 时，查询及子模型事件使用同一事务，并校验事务来源。`fresh_with` 接受点分隔路径，例如 `items.items`。同一直接关系的路径合并后统一加载，再对整批子模型加载下一层。普通 `refresh` 以原模型已加载的直接关系名为重载范围。

模型可用 `ModelRelations` 保存本实例已加载的关系。`relation_loaded` 对显式空值和空集合都返回 true，`get_relation` 可读取这些状态，`unset_relation` 将该项恢复为未加载。读取状态不会发起查询。`refreshable_names` 保留加载顺序，并跳过显式保存为 `Pivot` 的中间表模型。

`HasMany::load` 接收同类父模型切片、关系名和查询约束。它读取关联模型声明的表，用父模型键约束外键，并把结果作为 `Vec<Arc<Related>>` 写入各模型的关系状态。没有匹配的父模型也会得到一个已加载的空集合。关联模型经常规水合并触发 `retrieved`。关联模型未指定连接时，查询和水合均继承父模型的连接声明。查询约束可添加过滤或排序。

`HasOne::load` 使用同一批量查询方式，每个父模型保存首个匹配的 `Arc<Related>`。外键没有匹配行或父键为空时，关系状态为已加载的 `Null`。存在多行时可通过查询排序控制首个匹配。需要默认模型时，可用 `HasOne::load_with_default` 提供构造函数。框架为每个父模型传入已填外键的原始属性和关联连接，查得的首个匹配覆盖默认实例。默认实例不触发 `retrieved`。`of_many` 按列顺序在数据库聚合选一，默认用最大主键打破并列；单列 `of_many_by` 沿用同一聚合方向比较主键。`latest_of_many` 和 `oldest_of_many` 默认以关联主键为聚合列。聚合列全部为空的分组没有匹配。使用 `where_candidates` 可在每一步聚合前以 Query Builder 的参数绑定过滤候选行。加载时传入的查询约束仍用于外层结果。

`BelongsTo` 从当前模型的外键反查所属模型。默认用所属模型声明的主键，也可指定其他所属键。它在一条查询中加载各外键对应的模型，缺失或外键为空时标记为已加载 `Null`。如果自定义所属键存在重复值，字典保留查询顺序中的末项。`load_with_default` 可为未匹配的子模型提供默认所属模型。工厂接收关联原型、子模型和连接，默认所属模型不预填子模型的外键，也不触发 `retrieved`。

`associate` 经外键 setter 写入所属键，并缓存传入的模型。`associate_key` 直接设置外键后清除关系缓存。`dissociate` 将外键置空并保存已加载的空关系。外键 setter 失败时保留原有关系状态。这些方法只修改当前实例，需要持久化时再调用 `save`。

`BelongsToMany` 通过 `PivotKeys` 声明中间表及两侧键，一次联结查询加载关联模型和各自的 `Pivot`。默认选取中间表的两侧外键，可通过 `with_pivot` 追加列。框架从模型当前属性移除中间表投影，并写入默认名为 `pivot` 的关系，可用 `with_pivot_accessor` 改名。`load_in_transaction` 校验关联连接，并让联结查询、关联模型事件与后续关系共享该事务。`attach`、`update_existing_pivot` 和 `detach` 写入指定父模型的中间表记录，可传入同一连接签发的事务。额外属性不能改写关系键。`detach` 的 `None` 仅清除当前父模型的全部关系，空列表不执行删除。`sync` 在调用者事务内对比并写入关系，返回 `attached`、`detached`、`updated`。设置 `detaching=false` 会保留未列出的关联。`toggle` 移除输入中已关联的键，并按输入属性新增未关联的键，返回 `attached` 和 `detached`。两者的事务提交或错误回滚由调用者负责。`with_timestamps` 声明中间表的创建和更新列，两列同时加入读取投影。新增时从应用时钟填充时间，显式传值优先。直接更新时自动刷新更新时间，默认保留原创建时间。`sync` 遇到已有关系且没有提交属性时跳过更新。`with_touching_related` 使中间表写入后批量更新当前仍关联的模型时间戳。`sync` 和 `toggle` 在整次变更完成后触碰一次，遵循保存上下文及关联模型的时间戳禁用范围。`with_touching_parent` 对应关联模型反向关系声明的父侧触碰。写操作借用可变父模型，通过同一保存流程更新时间与模型事件，传入事务时该事务必须也属于父模型连接。`using::<P>()` 可指定自定义 Pivot。读取时按中间表的原始列构造已存在的模型，并安装到关系访问器下。自定义 Pivot 的新增将列值转换为模型原始属性，在父模型连接上走模型保存与事件。自定义更新从写连接按两侧键读取已有行，经模型批量赋值规则处理输入，只在属性有变化时走正常保存。`where_pivot_eq` 在联结读取和中间表变更时限定相等行；`with_pivot_value` 同时为新增行提供默认值，调用者可显式覆盖。自定义删除逐行派发 `deleting` 和 `deleted`，可通过 `deleting` 取消该行删除，并以原始关系键和可用主键限定记录。

## 模型保存事件前置

`fire_model_event` 使用当前应用的 Events。传入模型引用供 `eloquent.saving: 模型类` 等具名监听器使用，映射事件可先通过 `dispatch_mapped_model_event` 派发。写入前的 halt 模式仅将严格 `false` 视为取消。映射派发的空结果会回退到具名事件。完整保存、观察者装配及队列或后提交生命周期仍按后续契约交付。

## 插入前生成唯一 ID

模型可在元数据中声明 `uses_unique_ids` 和有序的 `unique_id_columns`。插入调用方可以为启用的模型调用 `attributes.set_unique_ids(&metadata, generator).await?`。该操作通过 getter 检查 PHP 空值，再经 setter 写入生成值，保留已有的非空 ID。启用 `db-uuid` 后，`new_uuid_v7` 可作为 UUID 生成器。应用声明字符串主键时应同时关闭自增并设置主键类型。ULID 内置生成、路由绑定校验和插入流程接线仍在后续范围。

## 保存时间戳前置

模型默认声明 `created_at` 与 `updated_at`，可为每列换名或用 `None` 关闭。持久化调用可用 `uses_timestamps()` 判断是否自动维护时间戳，或用 `without_timestamps_on` 在当前异步调用链临时关闭指定模型。直接调用 `update_timestamps` 使用一次应用时钟，保留已手工变更的字段，创建时间只在新记录上填充。Rust 模型类由显式标识区分，PHP 类继承传播和完整日期 cast 仍有对应缺口。此入口准备内存属性，保存事件与 SQL 写入由后续生命周期接入。

## 连接与持久身份

`attributes.key_for_save_query(&metadata)?` 为已有记录的保存或重查选择主键。属性基线中存在非 null 的字面主键时，保留数据库原始类型及值；基线主键缺失或为 null 时，读取当前模型主键的 getter/cast。主键声明包含点号时，仍按字面键查基线。此方法只选择值，完整保存、写入条件和事件仍按后续契约交付。

`ModelMetadata::resolve_connection` 按模型显式连接、领域 fallback、应用默认连接选择同一 `Databases` 服务；显式空名或 `"0"` 选应用默认，未知名报错。`ModelIdentity::model_key` 由模型属性机制产生，`MorphIdentity` 不把登录 ID 或 Guard 名当作模型类型。完整 morph map、连接生命周期与持久 save 仍须独立对应。

PostgreSQL 写入原始日期对象时，将查询语法格式化后的本地时间绑定为无时区 `timestamp` 参数。目标是 `timestamptz` 时，由数据库使用会话 `TimeZone` 解释。日期超出驱动可编码范围时返回 `model_postgres_date_binding_out_of_range`。PostgreSQL 二进制时间回读当前使用 ISO/UTC 表示，会话 `TimeZone` 和 `DateStyle` 对显示格式的影响仍待对应。各后端的运行验收分别取证。

固定来源：[Laravel 13.x Eloquent](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent.md) 与 [属性访问器和转换](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/eloquent-mutators.md)。完整官方章节仍按依赖逐项交付，当前例子仅说明可用的属性与身份前置。

预览的 original 是现有类型化实体可编辑字段的投影，不是数据库原始行字节水合。JSON比较沿PHP64：超出有符号64位整数范围的数会归为double，可能舍入相等；dirty不能充当任意精度大整数差异检测。预览不改变随后实际更新提交的Value，完整数值/后端对应仍须按范围验收。


需要为可覆盖的模型默认写入行为提供调用级 setter 时，使用 `ModelAttributes::set_attribute_with_default_setter`。同一次快照中已注册的自定义 setter 优先；仅缺省时调用传入的 `&dyn AttributeSetter`，可借用当前服务。两者均在锁外执行，成功补丁进入同一冲突检查；默认 setter 自行决定转换，框架不会随后再运行 cast。准备期间同名 setter 改变会拒绝过期补丁。这个入口不登记长期访问器，也不回滚 setter 的外部副作用。
