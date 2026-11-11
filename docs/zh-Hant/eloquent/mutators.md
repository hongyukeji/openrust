# Eloquent：修改器與型別轉換

## 存取器與修改器

在 `Model::attribute` 中宣告 `Attribute`，透過 `getAttributeValue` 和 `setAttributeValue` 套用。存取器接收原始值及目前原始屬性。修改器回傳要寫入的欄位。`fill` 檢查允許批次賦值的欄位後使用相同的修改器。

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

## 屬性型別轉換

在 `Model::casts` 中宣告內建轉換。布林、整數、浮點、字串和 JSON 的讀取保留 SQL 空值。存取器優先於內建轉換。JSON 賦值接受 `Value::Json`，純量值會編碼為 JSON。用 `getRawOriginal` 查看持久化快照，現有的 `getAttribute` 和 `setAttribute` 提供原始屬性讀寫。

### 精確小數與髒屬性

用 `AttributeCast::Decimal(scale)` 讀取按 HalfUp 規則捨入的定標小數字串。請以字串傳入精確小數，保留原始數字。模型的 `isDirty` 和 `getDirty` 會比較轉換後的值。轉換值等價時，`save` 略過該欄。發生變化的屬性按原始儲存值寫入。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn adjust_amount(record: &mut impl Model, amount: &str) -> DatabaseResult<(Option<Value>, bool)> {
    record.setAttributeValue("amount", amount)?;
    Ok((record.getAttributeValue("amount")?, record.isDirty()?))
}
```

### 日期轉換

用 `AttributeCast::DateTime` 讀取日期時間、用 `Date` 讀取本地日期的起點、用 `Timestamp` 讀取 Unix 秒數。日期與日期時間讀為帶微秒的 UTC RFC 3339 字串。寫入日期屬性時，使用應用程式連線的 `app.timezone` 和模型 `dateFormat` 指定的 Chrono 儲存格式，預設格式為 `%Y-%m-%d %H:%M:%S`。

```rust
use openrust::database::{DatabaseResult, Value, eloquent::Model};

pub fn change_occurred_at(record: &mut impl Model, date: &str) -> DatabaseResult<Option<Value>> {
    record.setAttributeValue("occurred_at", date)?;
    record.getAttributeValue("occurred_at")
}
```

### 查詢時轉換

用 `withCasts` 為目前查詢讀取或建立的模型設定轉換，可用於 `selected_at` 等查詢別名。同名欄位以查詢設定為準。用 `mergeCasts` 可修改單一模型的轉換規則。集合的 `fresh` 使用第一個模型的轉換設定並重新選取欄位，原查詢的別名需要重新選取才會出現。

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

### 列舉與自訂轉換

用 `EnumCast::of` 註冊列舉。`intoValue` 回傳成員名稱、字串後備值或整數後備值。使用 `getEnum::<Status>("status")` 和 `setEnum("status", Status::Published)` 讀寫 Rust 列舉。`getAttributeValue` 回傳正規化的後備值或成員名稱。設定為 `Value::Null` 可清除屬性。

實作 `CastsInboundAttributes` 處理賦值，實作 `CastsAttributes` 自訂讀取。`CastWrite::Value` 寫入單欄，`CastWrite::Patch` 寫入多欄。可用 `compare` 比較儲存值來判斷屬性是否變更。在 `Model::casts` 回傳 `CASTS`，也可把項目傳給 `withCasts`。

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

### 加密轉換

在 `Model::casts` 回傳 `CASTS`。轉換使用模型連線所屬應用程式的加密器。資料庫用 `TEXT` 欄儲存密文，透過 `getAttributeValue` 和 `setAttributeValue` 讀寫明文。SQL 空值保持為空。

`Array`、`Json` 和 `Object` 回傳 JSON 值，`Collection` 也可透過 `getEncryptedCollection` 讀取。設定 `APP_PREVIOUS_KEYS` 後可讀取舊金鑰寫入的資料。重新賦值並儲存時使用目前金鑰，單獨讀取會保留原密文。

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

### 列舉集合與值物件

用 `AsEnumCollection::of::<Status>()` 保留鍵，或用 `AsEnumArrayObject::of::<Status>()` 在賦值時重新編號。`getEnumCollection` 讀取型別化成員，`setEnumCollection` 寫入後備值或成員名稱。SQL 空值保持為空。 用 `editEnumCollection` 修改型別化集合並寫回模型。

為實作 `CastableObject` 的型別註冊 `ValueObjectCast::of::<T>()`。`getValueObject` 借用快取的值物件。`editValueObject` 修改副本並在驗證成功後更新模型欄位，呼叫 `save` 後持久化。範例假設模型已宣告 `left_code` 和 `enum_list` 的轉換。

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

### 加密類別轉換與 JSON 路徑

在加密 JSON 欄位宣告 `AsEncryptedArrayObject::cast()` 或 `AsEncryptedCollection::cast()`。用 `getEncryptedCollection` 讀取型別化集合，或用 `editEncryptedCollection` 修改。陣列物件解碼時需要陣列或有鍵物件。

用 `setAttributeValue("settings->notifications->enabled", true)` 或 `fillJsonAttribute` 更新巢狀值。模型更新整欄並保留同層欄位。加密欄位先解密，再寫入新密文。大量賦值時，在 `fillable` 中列出允許的每條路徑。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn enable_notifications(record: &mut impl Model) -> DatabaseResult<bool> {
    record.fillJsonAttribute("settings->notifications->enabled", true)?;
    record.save().await
}
```

### 轉換工廠

實作 `Castable::castUsing`，以型別化參數建構轉換器。用 `ClassCast::of::<C>(&ARGUMENTS)` 註冊雙向轉換，用 `ClassCast::inbound::<C>(&ARGUMENTS)` 註冊僅賦值轉換。參數使用靜態宣告，也可透過 `withCasts` 或 `mergeCasts` 選擇不同參數。

將 `CastableObject::WITHOUT_OBJECT_CACHING` 設為 `true`，可讓每次讀取重新建構值物件。`getValueObject` 透過 `Cow` 回傳借用值或自有值。

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

### 陣列物件與集合轉換

用 `AsArrayObject::cast()` 或 `AsCollection::cast()` 宣告 JSON 陣列或帶鍵物件。透過 `getJsonCollection`、`setJsonCollection` 和 `editJsonCollection` 保留鍵順序。預設賦值為空時寫入 JSON 字面值 `null`，用 `nullable()` 可寫入 SQL NULL。

用 `AsCollection::of::<Item>()` 透過 Serde 解析及投影各個元素。用 `AsCollection::map` 指定回呼，或用 `AsCollection::using` 在可選映射前建構集合。`getJsonCollectionAs::<Item>` 將投影後的元素讀為 Rust 型別。

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

### 二進位與向量轉換

在二進位欄位宣告 `AsBinary::uuid()` 或 `AsBinary::ulid()`。資料庫儲存 16 位元組，模型序列化回傳標準識別碼字串。自訂格式可將 `BinaryFormat::Custom(BinaryCodec::new(encode, decode))` 傳給 `AsBinary::of`。

用 `AsVector::cast()` 在 SQLite 或 MySQL 中以 JSON 文字儲存數值向量。轉換回傳數值陣列，也可讀取二進位欄位中的小端 float32 資料。原生向量 SQL 運算需要支援向量的驅動。

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

### 雜湊轉換

在模型的 `casts()` 中為 `password` 宣告 `AttributeCast::Hashed`。`create` 和 `update` 會在賦值時執行轉換。直接寫入明文時使用 `setAttributeValueAsync`，應用程式的雜湊服務會在執行緒池中運算，避免阻塞非同步執行器。

```rust
use openrust::database::{DatabaseResult, eloquent::Model};

pub async fn change_password(record: &mut impl Model, password: &str) -> DatabaseResult<bool> {
    record.setAttributeValueAsync("password", password).await?;
    record.save().await
}
```

既有雜湊通過目前驅動程式的設定驗證後原樣保留。`null` 直接通過，無需呼叫雜湊服務。同步賦值接受已驗證的雜湊和 `null`。將該欄位宣告為 `hidden` 可從模型 JSON 中省略它。

### 鏈式字串轉換

在模型的 `casts()` 中為 `label` 宣告 `AsStringable::cast()`。透過 `getValueObject` 讀取 `openrust::support::Stringable`。一般字串用 `setAttributeValue` 賦值，具型別的字串用 `setValueObject` 賦值。

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

賦值時保留空白，SQL `NULL` 讀取為 `None`。模型 JSON 輸出字串或 `null`。範例在儲存前明確裁剪具型別的字串。

### Fluent 屬性轉換

在模型的 `casts()` 中為 `options` 宣告 `AsFluent::cast()`。用 `Value::Json` 賦入陣列或物件，再透過 `getValueObject` 讀取 `openrust::support::Fluent`。`get` 支援點路徑，`value` 讀取字面鍵，`has` 能識別明確為 `null` 的鍵。

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

用 `editValueObject` 修改並寫回模型，再儲存。SQL `NULL` 讀取為 `None`。既有 JSON 純量或格式錯誤的內容在讀為 `Fluent` 時回傳錯誤。文字欄位保留寫入的鍵順序，原生 JSON 欄位遵循資料庫回傳的順序。

### URI 轉換

在 `Model::casts()` 中使用 `AsUri::cast()` 宣告屬性。文字賦值保留資料庫原文，讀取和序列化時解析並正規化 URI。SQL NULL 保持為空。支援絕對 URI 和相對參照。無效文字可以重新賦值修復。

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
