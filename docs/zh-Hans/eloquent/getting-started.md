# Eloquent 入门

## 生成模型类

运行 `./artisan make:model Flight` 生成 `app/models/flight.rs`。在 `app/models/mod.rs` 声明 `mod flight;`，并在 `app/mod.rs` 导出 `models` 模块后重新构建。使用 `--force` 可覆盖生成的文件。Rust 模型在 `ModelMetadata` 中声明数据表，每个实例持有一个 `ModelState`。

```rust
use openrust::database::eloquent::{Model, ModelMetadata, ModelState};

pub struct Flight(ModelState);

impl Model for Flight {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("flights")
            .fillable(&["name", "active"])
            .softDeletes("deleted_at");
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}
```

## 模型约定

默认主键为自增的 `id`，并维护 `created_at` 和 `updated_at`。在 `ModelMetadata` 中通过 `primaryKey`、`incrementing`、`timestamps` 或 `connection` 配置模型。使用 `fillable` 声明可批量赋值的字段。

## 查询模型

在应用请求或命令中使用 `Flight::query()?` 获取模型查询构造器，使用 `Flight::all().await?` 获取应用作用域后的模型集合，或用 `Flight::find(id).await?` 按主键查找。模型不存在时，`findOrFail` 返回 `DatabaseError::ModelNotFound`。可用 `queryOn` 指定数据库连接。

## 插入和更新模型

`create` 按批量赋值规则保存新模型。`setAttribute` 设置单个属性，`save` 用原主键写入已变更的属性。写入成功后同步模型快照。构造器的 `update` 和 `delete` 返回受影响行数。

## 删除模型

使用 `softDeletes("deleted_at")` 启用软删除。模型查询默认排除软删除记录，可用 `withTrashed` 或 `onlyTrashed` 查询它们，用 `restore` 清除删除时间，用 `forceDelete` 永久删除。

## 查询作用域

实现 `Scope<Model>` 可复用查询约束。用 `withScope` 应用它，或在模型的 `globalScopes` 注册全局作用域。使用 `withoutGlobalScope` 移除指定作用域。以下示例使用上面的 `Flight` 模型，数据表需包含已声明字段和时间戳。

```rust
use openrust::database::{
    Connection, DatabaseResult,
    eloquent::{Attributes, Builder, Scope},
};

struct Active;

impl Scope<Flight> for Active {
    fn apply(&self, query: &mut Builder<'_, Flight>) -> DatabaseResult<()> {
        query.r#where("active", "=", true);
        Ok(())
    }
}

pub async fn save_flight(connection: &Connection) -> DatabaseResult<()> {
    let mut flight = Flight::queryOn(connection)
        .create(Attributes::from([
            ("name".into(), "Flight 42".into()),
            ("active".into(), true.into()),
        ]))
        .await?;
    flight.setAttribute("name", "Flight 42A");
    flight.save().await?;

    let mut flights = Flight::queryOn(connection);
    flights.withScope(&Active)?;
    assert_eq!(flights.get().await?.len(), 1);

    flight.delete().await?;
    assert!(flight.trashed());
    flight.restore().await?;
    Ok(())
}
```
