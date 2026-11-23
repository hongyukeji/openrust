use std::sync::Arc;

use crate::database::eloquent::{Model, ModelKeyType, ModelState};

use super::{
    ColumnDefinition, DropColumnDefinition, ForeignIdColumnDefinition, ForeignKeyDefinition,
    IndexDefinition, MorphKeyType, SchemaDefaults, column_definition::ColumnType,
    index_definition::IndexType,
};

pub(super) enum Command {
    Column(ColumnDefinition),
    Index(IndexDefinition),
    Foreign(ForeignKeyDefinition),
    RenameColumn(String, String),
    TableComment(String),
    DropColumn(DropColumnDefinition),
    RenameIndex(String, String),
    DropIndex(IndexType, String),
    DropIndexColumns(IndexType, Vec<String>),
    DropForeign(String),
    DropForeignColumns(Vec<String>),
}

#[derive(Default)]
pub(in crate::database) struct TableOptions {
    pub(in crate::database) charset: Option<String>,
    pub(in crate::database) collation: Option<String>,
    pub(in crate::database) engine: Option<String>,
}

/// A table definition or a sequence of changes to an existing table.
#[derive(Default)]
pub struct Blueprint {
    pub(super) commands: Vec<Command>,
    pub(super) table: String,
    pub(super) prefix: Arc<str>,
    pub(super) creating: bool,
    pub(super) temporary: bool,
    pub(super) table_options: TableOptions,
    pub(super) defaults: SchemaDefaults,
    pub(super) after: Option<String>,
}

macro_rules! column_types {
    ($(($method:ident, $kind:ident)),+ $(,)?) => {
        $(
            pub fn $method(&mut self, name: &str) -> &mut ColumnDefinition {
                self.column(name, ColumnType::$kind)
            }
        )+
    };
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Blueprint {
    pub(super) fn has_online_index(&self) -> bool {
        self.commands
            .iter()
            .any(|command| matches!(command, Command::Index(index) if index.online))
    }
    fn column(&mut self, name: &str, kind: ColumnType) -> &mut ColumnDefinition {
        let mut column = ColumnDefinition::new(name, kind);
        if let Some(after) = &mut self.after {
            column.after(after);
            *after = name.to_owned();
        }
        self.commands.push(Command::Column(column));
        let Some(Command::Column(column)) = self.commands.last_mut() else {
            unreachable!("column was just appended")
        };
        column
    }

    column_types! {
        (tinyInteger, TinyInteger), (smallInteger, SmallInteger),
        (mediumInteger, MediumInteger), (integer, Integer), (bigInteger, BigInteger),
        (tinyText, TinyText), (text, Text), (mediumText, MediumText), (longText, LongText), (boolean, Boolean),
        (double, Double), (binary, Binary), (json, Json), (jsonb, Jsonb), (tsvector, TsVector),
        (date, Date), (year, Year), (ipAddress, IpAddress), (mac_address, MacAddress),
    }

    /// Returns the logical table reference supplied to the schema builder.
    pub fn get_table(&self) -> &str {
        &self.table
    }

    /// Returns the table prefix of the connection that owns this blueprint.
    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

    /// Returns whether this blueprint describes a new table.
    pub fn creating(&self) -> bool {
        self.creating
    }

    pub(super) fn has_explicit_primary(&self) -> bool {
        self.commands.iter().any(
            |command| matches!(command, Command::Index(index) if index.kind == IndexType::Primary),
        )
    }

    /// Iterates over pending column definitions in declaration order.
    pub fn get_columns(&self) -> impl Iterator<Item = &ColumnDefinition> {
        self.commands.iter().filter_map(|command| match command {
            Command::Column(column) => Some(column),
            _ => None,
        })
    }

    /// Iterates over pending columns for setting modifiers on helper-created columns.
    pub fn get_columns_mut(&mut self) -> impl Iterator<Item = &mut ColumnDefinition> {
        self.commands
            .iter_mut()
            .filter_map(|command| match command {
                Command::Column(column) => Some(column),
                _ => None,
            })
    }

    pub fn get_added_columns(&self) -> impl Iterator<Item = &ColumnDefinition> {
        self.get_columns().filter(|column| !column.change)
    }

    pub fn get_changed_columns(&self) -> impl Iterator<Item = &ColumnDefinition> {
        self.get_columns().filter(|column| column.change)
    }

    pub fn id(&mut self) -> &mut ColumnDefinition {
        self.id_with("id")
    }

    /// Declares an auto-incrementing primary key with a custom column name.
    pub fn id_with(&mut self, name: &str) -> &mut ColumnDefinition {
        self.big_increments(name)
    }

    /// Removes a pending column definition without issuing a drop statement.
    pub fn remove_column(&mut self, name: &str) -> &mut Self {
        self.commands
            .retain(|command| !matches!(command, Command::Column(column) if column.name == name));
        self
    }

    pub fn tiny_increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsigned_tiny_integer(name).auto_increment()
    }

    pub fn small_increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsigned_small_integer(name).auto_increment()
    }

    pub fn medium_increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsigned_medium_integer(name).auto_increment()
    }

    pub fn increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsigned_integer(name).auto_increment()
    }

    /// Declares an unsigned auto-incrementing integer, like `increments`.
    pub fn integer_increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.increments(name)
    }

    pub fn big_increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsigned_big_integer(name).auto_increment()
    }

    pub fn unsigned_tiny_integer(&mut self, name: &str) -> &mut ColumnDefinition {
        self.tinyInteger(name).unsigned()
    }

    pub fn unsigned_small_integer(&mut self, name: &str) -> &mut ColumnDefinition {
        self.smallInteger(name).unsigned()
    }

    pub fn unsigned_medium_integer(&mut self, name: &str) -> &mut ColumnDefinition {
        self.mediumInteger(name).unsigned()
    }

    pub fn unsigned_big_integer(&mut self, name: &str) -> &mut ColumnDefinition {
        self.bigInteger(name).unsigned()
    }

    pub fn unsigned_integer(&mut self, name: &str) -> &mut ColumnDefinition {
        self.integer(name).unsigned()
    }

    pub fn string(&mut self, name: &str, length: impl Into<Option<u32>>) -> &mut ColumnDefinition {
        let length = length
            .into()
            .filter(|length| *length != 0)
            .unwrap_or(self.defaults.string_length);
        self.column(name, ColumnType::String(length))
    }

    pub fn char(&mut self, name: &str, length: impl Into<Option<u32>>) -> &mut ColumnDefinition {
        let length = length.into().unwrap_or(self.defaults.string_length);
        self.column(name, ColumnType::Char(length))
    }

    pub fn r#enum(&mut self, name: &str, allowed: &[&str]) -> &mut ColumnDefinition {
        self.column(
            name,
            ColumnType::Enum(allowed.iter().map(|value| (*value).to_owned()).collect()),
        )
    }

    pub fn set(&mut self, name: &str, allowed: &[&str]) -> &mut ColumnDefinition {
        self.column(
            name,
            ColumnType::Set(allowed.iter().map(|value| (*value).to_owned()).collect()),
        )
    }

    /// A fixed or variable binary column. Zero selects the ordinary binary type.
    pub fn binary_with(&mut self, name: &str, length: u32, fixed: bool) -> &mut ColumnDefinition {
        if length == 0 {
            return self.binary(name);
        }
        self.column(name, ColumnType::SizedBinary(length, fixed))
    }

    /// Adds a spatial geometry column. The default SRID is zero.
    pub fn geometry(
        &mut self,
        name: &str,
        subtype: Option<&str>,
        srid: Option<u32>,
    ) -> &mut ColumnDefinition {
        let subtype = match subtype.map(str::to_ascii_lowercase).as_deref() {
            Some("point") => "point",
            Some("linestring") => "linestring",
            Some("polygon") => "polygon",
            Some("geometrycollection") => "geometrycollection",
            Some("multipoint") => "multipoint",
            Some("multilinestring") => "multilinestring",
            Some("multipolygon") => "multipolygon",
            _ => "geometry",
        };
        self.column(name, ColumnType::Spatial(subtype, srid.unwrap_or_default()))
    }

    /// Adds a spatial geography column. The default SRID is 4326.
    pub fn geography(
        &mut self,
        name: &str,
        subtype: Option<&str>,
        srid: Option<u32>,
    ) -> &mut ColumnDefinition {
        self.geometry(name, subtype, Some(srid.unwrap_or(4326)))
    }

    pub fn uuid(&mut self, name: &str) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Uuid)
    }

    /// Adds a native vector column, with an optional dimension on PostgreSQL.
    pub fn vector(&mut self, name: &str, dimensions: Option<u32>) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Vector(dimensions))
    }

    pub fn ulid(&mut self, name: &str) -> &mut ColumnDefinition {
        self.ulid_with(name, 26)
    }

    /// Defines a ULID column with an explicit character length.
    pub fn ulid_with(&mut self, name: &str, length: u32) -> &mut ColumnDefinition {
        self.char(name, length)
    }

    /// Places the callback's columns in order after an existing MySQL column.
    pub fn after(&mut self, column: &str, configure: impl FnOnce(&mut Self)) {
        let prior = self.after.replace(column.to_owned());
        configure(self);
        self.after = prior;
    }

    pub fn morphs(&mut self, name: &str) {
        self.morphs_with(name, None, None);
    }

    pub fn morphs_with(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, self.defaults.morph_key_type, false, index, after);
    }

    pub fn nullable_morphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, self.defaults.morph_key_type, true, index, after);
    }

    pub fn numeric_morphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Int, false, index, after);
    }

    pub fn nullable_numeric_morphs(
        &mut self,
        name: &str,
        index: Option<&str>,
        after: Option<&str>,
    ) {
        self.add_morphs(name, MorphKeyType::Int, true, index, after);
    }

    pub fn uuid_morphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Uuid, false, index, after);
    }

    pub fn nullable_uuid_morphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Uuid, true, index, after);
    }

    pub fn ulid_morphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Ulid, false, index, after);
    }

    pub fn nullable_ulid_morphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Ulid, true, index, after);
    }

    fn add_morphs(
        &mut self,
        name: &str,
        key_type: MorphKeyType,
        nullable: bool,
        index: Option<&str>,
        after: Option<&str>,
    ) {
        let kind = format!("{name}_type");
        let id = format!("{name}_id");
        let type_column = self.string(&kind, None);
        if nullable {
            type_column.nullable();
        }
        if let Some(after) = after {
            type_column.after(after);
        }

        let id_column = match key_type {
            MorphKeyType::Int => self.unsigned_big_integer(&id),
            MorphKeyType::Uuid => self.uuid(&id),
            MorphKeyType::Ulid => self.ulid(&id),
        };
        if nullable {
            id_column.nullable();
        }
        if after.is_some() {
            id_column.after(&kind);
        }
        self.index(&[&kind, &id], index);
    }

    /// Adds a floating-point column using default precision of 53 bits.
    pub fn float(&mut self, name: &str) -> &mut ColumnDefinition {
        self.float_with(name, 53)
    }

    /// Adds a floating-point column with explicit binary precision.
    pub fn float_with(&mut self, name: &str, precision: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Float(precision))
    }

    /// Defines a SQL Server computed column using a trusted SQL expression.
    pub fn computed(&mut self, name: &str, expression: impl Into<String>) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Computed)
            .virtual_as(expression)
    }

    /// Adds a column using a trusted, database-specific SQL definition.
    pub fn raw_column(
        &mut self,
        name: &str,
        definition: impl Into<String>,
    ) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Raw(definition.into()))
    }

    pub fn decimal(&mut self, name: &str, total: u8, places: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Decimal(total, places))
    }

    pub fn timestamp(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        let precision = precision.into().unwrap_or(self.defaults.time_precision);
        self.column(name, ColumnType::Timestamp(precision))
    }

    pub fn date_time(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        let precision = precision.into().unwrap_or(self.defaults.time_precision);
        self.column(name, ColumnType::DateTime(precision))
    }

    pub fn time(&mut self, name: &str, precision: impl Into<Option<u8>>) -> &mut ColumnDefinition {
        let precision = precision.into().unwrap_or(self.defaults.time_precision);
        self.column(name, ColumnType::Time(precision))
    }

    pub fn date_time_tz(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.date_time(name, precision)
    }

    pub fn time_tz(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.time(name, precision)
    }

    pub fn timestamp_tz(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.timestamp(name, precision)
    }

    pub fn timestamps(&mut self) {
        self.timestamps_with(None);
    }

    pub fn timestamps_with(&mut self, precision: impl Into<Option<u8>>) {
        let precision = precision.into();
        self.timestamp("created_at", precision).nullable();
        self.timestamp("updated_at", precision).nullable();
    }

    pub fn timestamps_tz(&mut self) {
        self.timestamps_tz_with(None);
    }

    pub fn timestamps_tz_with(&mut self, precision: impl Into<Option<u8>>) {
        self.timestamps_with(precision);
    }

    pub fn datetimes(&mut self, precision: impl Into<Option<u8>>) {
        let precision = precision.into();
        self.date_time("created_at", precision).nullable();
        self.date_time("updated_at", precision).nullable();
    }

    pub fn soft_deletes(&mut self) -> &mut ColumnDefinition {
        self.soft_deletes_with(None, None)
    }

    pub fn soft_deletes_with(
        &mut self,
        column: Option<&str>,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.timestamp(column.unwrap_or("deleted_at"), precision)
            .nullable()
    }

    pub fn soft_deletes_tz(&mut self) -> &mut ColumnDefinition {
        self.soft_deletes_tz_with(None, None)
    }

    pub fn soft_deletes_tz_with(
        &mut self,
        column: Option<&str>,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.soft_deletes_with(column, precision)
    }

    pub fn soft_deletes_datetime(
        &mut self,
        column: Option<&str>,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.date_time(column.unwrap_or("deleted_at"), precision)
            .nullable()
    }

    pub fn remember_token(&mut self) -> &mut ColumnDefinition {
        self.string("remember_token", 100).nullable()
    }

    pub fn nullable_timestamps(&mut self) {
        self.timestamps();
    }

    pub fn nullable_timestamps_tz(&mut self) {
        self.timestamps_tz();
    }

    pub fn foreign_id(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.unsigned_big_integer(name);
        ForeignIdColumnDefinition::new(self, name)
    }

    pub fn foreign_uuid(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.uuid(name);
        ForeignIdColumnDefinition::new(self, name)
    }

    pub fn foreign_ulid(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.foreign_ulid_with(name, 26)
    }

    /// Defines a foreign ULID column with an explicit character length.
    pub fn foreign_ulid_with(&mut self, name: &str, length: u32) -> ForeignIdColumnDefinition<'_> {
        self.ulid_with(name, length);
        ForeignIdColumnDefinition::new(self, name)
    }

    pub fn foreign_id_for<M: Model>(
        &mut self,
        column: Option<&str>,
    ) -> ForeignIdColumnDefinition<'_> {
        let model = M::from_state(ModelState::default());
        let key_type = model.get_key_type();
        self.foreign_model_id(&model, column, key_type)
    }

    pub fn foreign_uuid_for<M: Model>(
        &mut self,
        column: Option<&str>,
    ) -> ForeignIdColumnDefinition<'_> {
        let model = M::from_state(ModelState::default());
        self.foreign_model_id(&model, column, ModelKeyType::Uuid)
    }

    pub fn foreign_ulid_for<M: Model>(
        &mut self,
        column: Option<&str>,
    ) -> ForeignIdColumnDefinition<'_> {
        let model = M::from_state(ModelState::default());
        self.foreign_model_id(&model, column, ModelKeyType::Ulid)
    }

    fn foreign_model_id(
        &mut self,
        model: &impl Model,
        column: Option<&str>,
        key_type: ModelKeyType,
    ) -> ForeignIdColumnDefinition<'_> {
        let name = column.map_or_else(|| model.get_foreign_key(), str::to_owned);
        let definition = match key_type {
            ModelKeyType::Integer => self.foreign_id(&name),
            ModelKeyType::Uuid | ModelKeyType::String => self.foreign_uuid(&name),
            ModelKeyType::Ulid => self.foreign_ulid(&name),
        };
        definition.for_model(model.get_table(), model.get_key_name())
    }

    pub fn foreign(&mut self, columns: &[&str], name: Option<&str>) -> &mut ForeignKeyDefinition {
        self.commands
            .push(Command::Foreign(ForeignKeyDefinition::new(columns, name)));
        let Some(Command::Foreign(foreign)) = self.commands.last_mut() else {
            unreachable!("foreign key was just appended")
        };
        foreign
    }

    fn add_index(
        &mut self,
        kind: IndexType,
        columns: &[&str],
        name: Option<&str>,
    ) -> &mut IndexDefinition {
        self.commands
            .push(Command::Index(IndexDefinition::new(kind, columns, name)));
        let Some(Command::Index(index)) = self.commands.last_mut() else {
            unreachable!("index was just appended")
        };
        index
    }

    pub fn index(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::Index, columns, name)
    }

    /// Adds a named index using trusted, database-specific SQL for the key list.
    pub fn raw_index(&mut self, expression: &str, name: &str) -> &mut IndexDefinition {
        let index = self.index(&[], Some(name));
        index.raw_expression = Some(expression.to_owned());
        index
    }

    pub fn unique(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::Unique, columns, name)
    }

    pub fn primary(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::Primary, columns, name)
    }

    pub fn full_text(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::FullText, columns, name)
    }

    pub fn spatial_index(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::Spatial, columns, name)
    }

    pub fn vector_index(&mut self, column: &str, name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::Vector, &[column], name)
    }

    pub fn charset(&mut self, charset: &str) {
        self.table_options.charset = Some(charset.to_owned());
    }

    pub fn collation(&mut self, collation: &str) {
        self.table_options.collation = Some(collation.to_owned());
    }

    pub fn engine(&mut self, engine: &str) {
        self.table_options.engine = Some(engine.to_owned());
    }

    pub fn inno_db(&mut self) {
        self.engine("InnoDB");
    }

    pub fn comment(&mut self, comment: impl Into<String>) {
        self.commands.push(Command::TableComment(comment.into()));
    }

    pub fn temporary(&mut self) {
        self.temporary = true;
    }

    pub fn rename_column(&mut self, from: &str, to: &str) {
        self.commands
            .push(Command::RenameColumn(from.into(), to.into()));
    }

    pub fn drop_column(&mut self, columns: &[&str]) -> &mut DropColumnDefinition {
        self.commands
            .push(Command::DropColumn(DropColumnDefinition::new(columns)));
        let Some(Command::DropColumn(drop)) = self.commands.last_mut() else {
            unreachable!("drop column command was just appended")
        };
        drop
    }

    pub fn drop_morphs(&mut self, name: &str, index: Option<&str>) {
        let kind = format!("{name}_type");
        let id = format!("{name}_id");
        if let Some(index) = index {
            self.drop_index(index);
        } else {
            self.drop_index_columns(&[&kind, &id]);
        }
        self.drop_column(&[&kind, &id]);
    }

    pub fn drop_timestamps(&mut self) {
        self.drop_column(&["created_at", "updated_at"]);
    }

    pub fn drop_timestamps_tz(&mut self) {
        self.drop_timestamps();
    }

    pub fn drop_soft_deletes(&mut self, column: Option<&str>) {
        self.drop_column(&[column.unwrap_or("deleted_at")]);
    }

    pub fn drop_soft_deletes_tz(&mut self, column: Option<&str>) {
        self.drop_soft_deletes(column);
    }

    pub fn drop_remember_token(&mut self) {
        self.drop_column(&["remember_token"]);
    }

    pub fn drop_constrained_foreign_id(&mut self, column: &str) {
        self.drop_foreign_columns(&[column]);
        self.drop_column(&[column]);
    }

    pub fn drop_foreign_id_for<M: Model>(&mut self, column: Option<&str>) {
        let name = column.map_or_else(
            || M::from_state(ModelState::default()).get_foreign_key(),
            str::to_owned,
        );
        self.drop_column(&[&name]);
    }

    pub fn drop_constrained_foreign_id_for<M: Model>(&mut self, column: Option<&str>) {
        let name = column.map_or_else(
            || M::from_state(ModelState::default()).get_foreign_key(),
            str::to_owned,
        );
        self.drop_constrained_foreign_id(&name);
    }

    pub fn rename_index(&mut self, from: &str, to: &str) {
        self.commands
            .push(Command::RenameIndex(from.into(), to.into()));
    }

    pub fn drop_index(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Index, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn drop_index_columns(&mut self, columns: &[&str]) {
        self.drop_index_columns_of_type(IndexType::Index, columns);
    }

    pub fn drop_unique(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Unique, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn drop_unique_columns(&mut self, columns: &[&str]) {
        self.drop_index_columns_of_type(IndexType::Unique, columns);
    }

    pub fn drop_primary(&mut self) {
        self.commands
            .push(Command::DropIndex(IndexType::Primary, String::new()));
    }

    /// Drops a primary key by its constraint name on SQL Server.
    pub fn drop_primary_named(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Primary, name.into()));
    }

    /// Drops the primary key named by the table and ordered columns on SQL Server.
    pub fn drop_primary_columns(&mut self, columns: &[&str]) {
        self.drop_index_columns_of_type(IndexType::Primary, columns);
    }

    pub fn drop_full_text(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::FullText, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn drop_full_text_columns(&mut self, columns: &[&str]) {
        self.drop_index_columns_of_type(IndexType::FullText, columns);
    }

    pub fn drop_spatial_index(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Spatial, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn drop_spatial_index_columns(&mut self, columns: &[&str]) {
        self.drop_index_columns_of_type(IndexType::Spatial, columns);
    }

    pub fn drop_vector_index(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Vector, name.into()));
    }

    pub fn drop_vector_index_columns(&mut self, columns: &[&str]) {
        self.drop_index_columns_of_type(IndexType::Vector, columns);
    }

    fn drop_index_columns_of_type(&mut self, kind: IndexType, columns: &[&str]) {
        self.commands.push(Command::DropIndexColumns(
            kind,
            columns.iter().map(|column| (*column).to_owned()).collect(),
        ));
    }

    pub fn drop_foreign_columns(&mut self, columns: &[&str]) {
        self.commands.push(Command::DropForeignColumns(
            columns.iter().map(|column| (*column).to_owned()).collect(),
        ));
    }

    pub fn drop_foreign(&mut self, name: &str) {
        self.commands.push(Command::DropForeign(name.into()));
    }
}
