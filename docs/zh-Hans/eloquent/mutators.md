# Eloquent：修改器与类型转换

## 访问器与修改器

在 `Model::attribute` 中声明 `Attribute`，通过 `getAttributeValue` 和 `setAttributeValue` 应用它。访问器接收原始值及当前原始属性。修改器返回要写入的列。`fill` 检查允许批量赋值的字段后使用相同的修改器。

```rust
fn attribute(name: &str) -> Option<Attribute> {
    if name == "label" {
        return Some(Attribute::get(|value, _| match value {
            Some(Value::Text(label)) => Ok(label.to_uppercase().into()),
            _ => Ok(Value::Null),
        }));
    }
    None
}
```

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn set_record_label(record: &mut impl Model, label: &str) -> DatabaseResult<Option<Value>> {
    record.setAttributeValue("label", label)?;
    record.getAttributeValue("label")
}
```

## 属性类型转换

在 `Model::casts` 中声明内置转换。布尔、整数、浮点、字符串和 JSON 的读取保留 SQL 空值。访问器优先于内置转换。JSON 赋值接受 `Value::Json`，标量值会编码为 JSON。用 `getRawOriginal` 查看持久化快照，现有的 `getAttribute` 和 `setAttribute` 提供原始属性读写。

### 精确小数与脏属性

用 `AttributeCast::Decimal(scale)` 读取按 HalfUp 规则舍入的定标小数字符串。请以字符串传入精确小数，保留原始数字。模型的 `isDirty` 和 `getDirty` 会比较转换后的值。转换值等价时，`save` 跳过该列。发生变化的属性按原始存储值写入。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn adjust_amount(record: &mut impl Model, amount: &str) -> DatabaseResult<(Option<Value>, bool)> {
    record.setAttributeValue("amount", amount)?;
    Ok((record.getAttributeValue("amount")?, record.isDirty()?))
}
```

### 日期转换

用 `AttributeCast::DateTime` 读取日期时间、用 `Date` 读取本地日期的起点、用 `Timestamp` 读取 Unix 秒数。日期与日期时间读为带微秒的 UTC RFC 3339 字符串。写入日期属性时，使用应用连接的 `app.timezone` 和模型 `dateFormat` 指定的 Chrono 存储格式，默认格式为 `%Y-%m-%d %H:%M:%S`。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn change_occurred_at(record: &mut impl Model, date: &str) -> DatabaseResult<Option<Value>> {
    record.setAttributeValue("occurred_at", date)?;
    record.getAttributeValue("occurred_at")
}
```

### 查询时转换

用 `withCasts` 为当前查询读取或创建的模型配置转换，可用于 `selected_at` 等查询别名。同名字段以查询配置为准。用 `mergeCasts` 可修改单个模型的转换规则。集合的 `fresh` 使用第一个模型的转换配置并重新选取列，原查询的别名需要重新选取才会出现。

```rust
use openrust::database::{
    Connection, DatabaseResult, Value,
    eloquent::{Model, casts::AttributeCast},
    query::Expression,
};

pub async fn selected_date<M: Model>(db: &Connection) -> DatabaseResult<Option<Value>> {
    let Some(record) = M::queryOn(db)
        .select([Expression::new("occurred_at AS selected_at")])
        .withCasts([("selected_at", AttributeCast::DateTime)])
        .first()
        .await?
    else {
        return Ok(None);
    };
    record.getAttributeValue("selected_at")
}
```

### 枚举与自定义转换

用 `EnumCast::of` 注册枚举。`intoValue` 返回成员名称、字符串后备值或整数后备值。使用 `getEnum::<Status>("status")` 和 `setEnum("status", Status::Published)` 读写 Rust 枚举。`getAttributeValue` 返回规范化的后备值或成员名称。赋值为 `Value::Null` 可清空属性。

实现 `CastsInboundAttributes` 处理赋值，实现 `CastsAttributes` 自定义读取。`CastWrite::Value` 写入单列，`CastWrite::Patch` 写入多列。可用 `compare` 比较存储值来判断属性是否变更。在 `Model::casts` 返回 `CASTS`，也可把条目传给 `withCasts`。

```rust
use openrust::database::{
    DatabaseError, DatabaseResult, Value,
    eloquent::{
        Attributes,
        casts::{AttributeCast, CastWrite, CastableEnum, CastsInboundAttributes, EnumCast},
    },
};

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Draft,
    Published,
}

impl CastableEnum for Status {
    fn fromValue(value: &Value) -> DatabaseResult<Self> {
        match value {
            Value::Text(name) if name == "Draft" => Ok(Self::Draft),
            Value::Text(name) if name == "Published" => Ok(Self::Published),
            _ => Err(DatabaseError::InvalidQuery("status case")),
        }
    }

    fn intoValue(self) -> Value {
        match self {
            Self::Draft => "Draft".into(),
            Self::Published => "Published".into(),
        }
    }
}

struct Trimmed;

impl CastsInboundAttributes for Trimmed {
    fn set(&self, _: &str, value: Value, _: &Attributes) -> DatabaseResult<CastWrite> {
        match value {
            Value::Text(text) => Ok(CastWrite::Value(text.trim().into())),
            Value::Null => Ok(CastWrite::Value(Value::Null)),
            _ => Err(DatabaseError::InvalidQuery("code must be text")),
        }
    }
}

pub const CASTS: &[(&str, AttributeCast)] = &[
    ("status", AttributeCast::Enum(EnumCast::of::<Status>())),
    ("inbound_code", AttributeCast::Inbound(&Trimmed)),
];
```

### 加密转换

在 `Model::casts` 返回 `CASTS`。转换使用模型连接所属应用的加密器。数据库用 `TEXT` 列存放密文，通过 `getAttributeValue` 和 `setAttributeValue` 读写明文。SQL 空值保持为空。

`Array`、`Json` 和 `Object` 返回 JSON 值，`Collection` 还可以通过 `getEncryptedCollection` 读取。配置 `APP_PREVIOUS_KEYS` 后可读取旧密钥写入的数据。重新赋值并保存时使用当前密钥，单独读取会保留原密文。

```rust
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Model, casts::{AttributeCast, EncryptedCast}},
};

pub const CASTS: &[(&str, AttributeCast)] = &[
    ("secret", AttributeCast::Encrypted(EncryptedCast::String)),
    ("settings", AttributeCast::Encrypted(EncryptedCast::Array)),
];

pub async fn change_secret(
    model: &mut impl Model,
    secret: &str,
) -> DatabaseResult<Option<Value>> {
    model.setAttributeValue("secret", secret)?;
    model.save().await?;
    model.getAttributeValue("secret")
}
```

### 枚举集合与值对象

用 `AsEnumCollection::of::<Status>()` 保留键，或用 `AsEnumArrayObject::of::<Status>()` 在赋值时重新编号。`getEnumCollection` 读取类型化成员，`setEnumCollection` 写入后备值或成员名称。SQL 空值保持为空。 用 `editEnumCollection` 修改类型化集合并写回模型。

为实现 `CastableObject` 的类型注册 `ValueObjectCast::of::<T>()`。`getValueObject` 借用缓存的值对象。`editValueObject` 修改副本并在验证成功后更新模型列，调用 `save` 后持久化。示例假设模型已声明 `left_code` 和 `enum_list` 的转换。

```rust
use openrust::{
    database::{
        DatabaseResult,
        eloquent::{Model, casts::{CastableEnum, CastableObject}},
    },
    support::Collection,
};

pub async fn update_codes<M: Model, T: CastableObject>(
    record: &mut M,
    edit: impl FnOnce(&mut T) -> DatabaseResult<()>,
) -> DatabaseResult<bool> {
    record.editValueObject("left_code", edit)?;
    record.save().await
}

pub async fn update_enum_list<M: Model, E: CastableEnum>(
    record: &mut M,
    values: Collection<E>,
) -> DatabaseResult<bool> {
    record.setEnumCollection("enum_list", values)?;
    record.save().await
}
```

### 加密类转换与 JSON 路径

在加密 JSON 列声明 `AsEncryptedArrayObject::cast()` 或 `AsEncryptedCollection::cast()`。用 `getEncryptedCollection` 读取类型化集合，或用 `editEncryptedCollection` 修改。数组对象解码时要求数组或带键对象。

用 `setAttributeValue("settings->notifications->enabled", true)` 或 `fillJsonAttribute` 更新嵌套值。模型更新整列并保留同级字段。加密列先解密，再写入新密文。批量赋值时，在 `fillable` 中列出允许的每条路径。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn enable_notifications(record: &mut impl Model) -> DatabaseResult<bool> {
    record.fillJsonAttribute("settings->notifications->enabled", true)?;
    record.save().await
}
```

### 转换工厂

实现 `Castable::castUsing`，用类型化参数构造转换器。用 `ClassCast::of::<C>(&ARGUMENTS)` 注册双向转换，用 `ClassCast::inbound::<C>(&ARGUMENTS)` 注册仅赋值转换。参数使用静态声明，也可通过 `withCasts` 或 `mergeCasts` 选择不同的参数。

将 `CastableObject::WITHOUT_OBJECT_CACHING` 设为 `true`，可让每次读取重新构造值对象。`getValueObject` 通过 `Cow` 返回借用或自有值。

```rust
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, casts::{Castable, CastsAttributes, ClassCast}},
};

pub async fn save_scaled<C: Castable<Arguments = i64>>(
    record: &mut impl Model,
    factor: &'static i64,
    value: i64,
) -> DatabaseResult<bool>
where
    C::Caster: CastsAttributes,
{
    record.mergeCasts([("factory_value", ClassCast::of::<C>(factor))]);
    record.setAttributeValue("factory_value", value)?;
    record.save().await
}
```

### 数组对象与集合转换

用 `AsArrayObject::cast()` 或 `AsCollection::cast()` 声明 JSON 数组或带键对象。通过 `getJsonCollection`、`setJsonCollection` 和 `editJsonCollection` 保留键顺序。默认赋值为空时写入 JSON 字面值 `null`，用 `nullable()` 可写入 SQL NULL。

用 `AsCollection::of::<Item>()` 通过 Serde 解析和投影各个元素。用 `AsCollection::map` 指定回调，或用 `AsCollection::using` 在可选映射前构造集合。`getJsonCollectionAs::<Item>` 将投影后的元素读为 Rust 类型。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn add_option(record: &mut impl Model) -> DatabaseResult<bool> {
    record.editJsonCollection("json_collection", |items| {
        items.put("enabled", serde_json::json!(true));
        Ok(())
    })?;
    record.save().await
}
```

### 二进制与向量转换

在二进制列上声明 `AsBinary::uuid()` 或 `AsBinary::ulid()`。数据库存储 16 字节，模型序列化返回规范标识符字符串。自定义格式可将 `BinaryFormat::Custom(BinaryCodec::new(encode, decode))` 传给 `AsBinary::of`。

用 `AsVector::cast()` 在 SQLite 或 MySQL 中以 JSON 文本存储数值向量。转换返回数值数组，也可读取二进制列里的小端 float32 数据。原生向量 SQL 运算需要支持向量的驱动。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub async fn store_embedding(
    record: &mut impl Model,
    uuid: &str,
    embedding: serde_json::Value,
) -> DatabaseResult<bool> {
    record.setAttributeValue("binary_uuid", uuid)?;
    record.setAttributeValue("embedding", Value::Json(embedding))?;
    record.save().await
}
```

### 哈希转换

在模型的 `casts()` 中为 `password` 声明 `AttributeCast::Hashed`。`create` 和 `update` 会在赋值时执行转换。直接写入明文时使用 `setAttributeValueAsync`，应用的哈希服务会在线程池中计算，避免阻塞异步执行器。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn change_password(record: &mut impl Model, password: &str) -> DatabaseResult<bool> {
    record.setAttributeValueAsync("password", password).await?;
    record.save().await
}
```

已有哈希通过当前驱动的配置校验后原样保留。`null` 直接通过，无需调用哈希服务。同步赋值接受已校验的哈希和 `null`。将该字段声明为 `hidden` 可从模型 JSON 中省略它。

### 链式字符串转换

在模型的 `casts()` 中为 `label` 声明 `AsStringable::cast()`。通过 `getValueObject` 读取 `openrust::support::Stringable`。普通字符串用 `setAttributeValue` 赋值，类型化字符串用 `setValueObject` 赋值。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    support::Stringable,
};

pub async fn trim_label(record: &mut impl Model) -> DatabaseResult<bool> {
    let Some(label) = record.getValueObject::<Stringable>("label")? else {
        return Ok(false);
    };
    let label = label.into_owned().trim();
    record.setValueObject("label", Some(label))?;
    record.save().await
}
```

赋值时保留空白，SQL `NULL` 读取为 `None`。模型 JSON 输出字符串或 `null`。示例在保存前显式裁剪类型化字符串。

### Fluent 属性转换

在模型的 `casts()` 中为 `options` 声明 `AsFluent::cast()`。用 `Value::Json` 赋入数组或对象，再通过 `getValueObject` 读取 `openrust::support::Fluent`。`get` 支持点路径，`value` 读取字面键，`has` 能识别显式为 `null` 的键。

```rust
use openrust::{
    database::{DatabaseError, DatabaseResult, eloquent::Model},
    support::Fluent,
};

pub async fn enable_feature(record: &mut impl Model) -> DatabaseResult<bool> {
    record.editValueObject::<Fluent>("options", |options| {
        options
            .set("features.preview", serde_json::json!(true))
            .map_err(|_| DatabaseError::InvalidQuery("options path"))?;
        Ok(())
    })?;
    record.save().await
}
```

用 `editValueObject` 修改并写回模型，再保存。SQL `NULL` 读取为 `None`。已有 JSON 标量或格式错误的正文在读为 `Fluent` 时返回错误。文本列保留写入的键顺序，原生 JSON 列遵循数据库返回的顺序。

### URI 转换

在 `Model::casts()` 中使用 `AsUri::cast()` 声明属性。文本赋值保留数据库原文，读取和序列化时解析并规范化 URI。SQL NULL 保持为空。支持绝对 URI 和相对引用。无效文本可以重新赋值修复。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    support::Uri,
};

fn update_link(model: &mut impl Model) -> DatabaseResult<Option<String>> {
    let previous = model
        .getValueObject::<Uri>("link")?
        .map(|uri| uri.to_string());
    model.setValueObject("link", Some(Uri::of("../orders?state=open").unwrap()))?;
    Ok(previous)
}
```
