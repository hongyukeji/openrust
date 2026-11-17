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

#[allow(non_snake_case, reason = "Matches Laravel's Blueprint API")]
impl Blueprint {
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
        (double, Double), (binary, Binary), (json, Json), (jsonb, Jsonb),
        (date, Date), (year, Year), (ipAddress, IpAddress), (macAddress, MacAddress),
    }

    /// Returns the logical table reference supplied to the schema builder.
    pub fn getTable(&self) -> &str {
        &self.table
    }

    /// Returns the table prefix of the connection that owns this blueprint.
    pub fn getPrefix(&self) -> &str {
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
    pub fn getColumns(&self) -> impl Iterator<Item = &ColumnDefinition> {
        self.commands.iter().filter_map(|command| match command {
            Command::Column(column) => Some(column),
            _ => None,
        })
    }

    pub fn getAddedColumns(&self) -> impl Iterator<Item = &ColumnDefinition> {
        self.getColumns().filter(|column| !column.change)
    }

    pub fn getChangedColumns(&self) -> impl Iterator<Item = &ColumnDefinition> {
        self.getColumns().filter(|column| column.change)
    }

    pub fn id(&mut self) -> &mut ColumnDefinition {
        self.idWith("id")
    }

    /// Declares an auto-incrementing primary key with a custom column name.
    pub fn idWith(&mut self, name: &str) -> &mut ColumnDefinition {
        self.bigIncrements(name)
    }

    /// Removes a pending column definition without issuing a drop statement.
    pub fn removeColumn(&mut self, name: &str) -> &mut Self {
        self.commands
            .retain(|command| !matches!(command, Command::Column(column) if column.name == name));
        self
    }

    pub fn tinyIncrements(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsignedTinyInteger(name).autoIncrement()
    }

    pub fn smallIncrements(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsignedSmallInteger(name).autoIncrement()
    }

    pub fn mediumIncrements(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsignedMediumInteger(name).autoIncrement()
    }

    pub fn increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsignedInteger(name).autoIncrement()
    }

    /// Declares an unsigned auto-incrementing integer, like `increments`.
    pub fn integerIncrements(&mut self, name: &str) -> &mut ColumnDefinition {
        self.increments(name)
    }

    pub fn bigIncrements(&mut self, name: &str) -> &mut ColumnDefinition {
        self.unsignedBigInteger(name).autoIncrement()
    }

    pub fn unsignedTinyInteger(&mut self, name: &str) -> &mut ColumnDefinition {
        self.tinyInteger(name).unsigned()
    }

    pub fn unsignedSmallInteger(&mut self, name: &str) -> &mut ColumnDefinition {
        self.smallInteger(name).unsigned()
    }

    pub fn unsignedMediumInteger(&mut self, name: &str) -> &mut ColumnDefinition {
        self.mediumInteger(name).unsigned()
    }

    pub fn unsignedBigInteger(&mut self, name: &str) -> &mut ColumnDefinition {
        self.bigInteger(name).unsigned()
    }

    pub fn unsignedInteger(&mut self, name: &str) -> &mut ColumnDefinition {
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
    pub fn binaryWith(&mut self, name: &str, length: u32, fixed: bool) -> &mut ColumnDefinition {
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
        self.ulidWith(name, 26)
    }

    /// Defines a ULID column with an explicit character length.
    pub fn ulidWith(&mut self, name: &str, length: u32) -> &mut ColumnDefinition {
        self.char(name, length)
    }

    /// Places the callback's columns in order after an existing MySQL column.
    pub fn after(&mut self, column: &str, configure: impl FnOnce(&mut Self)) {
        let prior = self.after.replace(column.to_owned());
        configure(self);
        self.after = prior;
    }

    pub fn morphs(&mut self, name: &str) {
        self.morphsWith(name, None, None);
    }

    pub fn morphsWith(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, self.defaults.morph_key_type, false, index, after);
    }

    pub fn nullableMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, self.defaults.morph_key_type, true, index, after);
    }

    pub fn numericMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Int, false, index, after);
    }

    pub fn nullableNumericMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Int, true, index, after);
    }

    pub fn uuidMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Uuid, false, index, after);
    }

    pub fn nullableUuidMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Uuid, true, index, after);
    }

    pub fn ulidMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
        self.add_morphs(name, MorphKeyType::Ulid, false, index, after);
    }

    pub fn nullableUlidMorphs(&mut self, name: &str, index: Option<&str>, after: Option<&str>) {
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
            MorphKeyType::Int => self.unsignedBigInteger(&id),
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

    /// Adds a floating-point column using Laravel's default precision of 53 bits.
    pub fn float(&mut self, name: &str) -> &mut ColumnDefinition {
        self.floatWith(name, 53)
    }

    /// Adds a floating-point column with explicit binary precision.
    pub fn floatWith(&mut self, name: &str, precision: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Float(precision))
    }

    /// Adds a column using a trusted, database-specific SQL definition.
    pub fn rawColumn(
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

    pub fn dateTime(
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

    pub fn dateTimeTz(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.dateTime(name, precision)
    }

    pub fn timeTz(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.time(name, precision)
    }

    pub fn timestampTz(
        &mut self,
        name: &str,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.timestamp(name, precision)
    }

    pub fn timestamps(&mut self) {
        self.timestampsWith(None);
    }

    pub fn timestampsWith(&mut self, precision: impl Into<Option<u8>>) {
        let precision = precision.into();
        self.timestamp("created_at", precision).nullable();
        self.timestamp("updated_at", precision).nullable();
    }

    pub fn timestampsTz(&mut self) {
        self.timestampsTzWith(None);
    }

    pub fn timestampsTzWith(&mut self, precision: impl Into<Option<u8>>) {
        self.timestampsWith(precision);
    }

    pub fn datetimes(&mut self, precision: impl Into<Option<u8>>) {
        let precision = precision.into();
        self.dateTime("created_at", precision).nullable();
        self.dateTime("updated_at", precision).nullable();
    }

    pub fn softDeletes(&mut self) -> &mut ColumnDefinition {
        self.softDeletesWith(None, None)
    }

    pub fn softDeletesWith(
        &mut self,
        column: Option<&str>,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.timestamp(column.unwrap_or("deleted_at"), precision)
            .nullable()
    }

    pub fn softDeletesTz(&mut self) -> &mut ColumnDefinition {
        self.softDeletesTzWith(None, None)
    }

    pub fn softDeletesTzWith(
        &mut self,
        column: Option<&str>,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.softDeletesWith(column, precision)
    }

    pub fn softDeletesDatetime(
        &mut self,
        column: Option<&str>,
        precision: impl Into<Option<u8>>,
    ) -> &mut ColumnDefinition {
        self.dateTime(column.unwrap_or("deleted_at"), precision)
            .nullable()
    }

    pub fn rememberToken(&mut self) -> &mut ColumnDefinition {
        self.string("remember_token", 100).nullable()
    }

    pub fn nullableTimestamps(&mut self) {
        self.timestamps();
    }

    pub fn nullableTimestampsTz(&mut self) {
        self.timestampsTz();
    }

    pub fn foreignId(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.unsignedBigInteger(name);
        ForeignIdColumnDefinition::new(self, name)
    }

    pub fn foreignUuid(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.uuid(name);
        ForeignIdColumnDefinition::new(self, name)
    }

    pub fn foreignUlid(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.foreignUlidWith(name, 26)
    }

    /// Defines a foreign ULID column with an explicit character length.
    pub fn foreignUlidWith(&mut self, name: &str, length: u32) -> ForeignIdColumnDefinition<'_> {
        self.ulidWith(name, length);
        ForeignIdColumnDefinition::new(self, name)
    }

    pub fn foreignIdFor<M: Model>(
        &mut self,
        column: Option<&str>,
    ) -> ForeignIdColumnDefinition<'_> {
        let model = M::from_state(ModelState::default());
        let key_type = model.getKeyType();
        self.foreign_model_id(&model, column, key_type)
    }

    pub fn foreignUuidFor<M: Model>(
        &mut self,
        column: Option<&str>,
    ) -> ForeignIdColumnDefinition<'_> {
        let model = M::from_state(ModelState::default());
        self.foreign_model_id(&model, column, ModelKeyType::Uuid)
    }

    pub fn foreignUlidFor<M: Model>(
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
        let name = column.map_or_else(|| model.getForeignKey(), str::to_owned);
        let definition = match key_type {
            ModelKeyType::Integer => self.foreignId(&name),
            ModelKeyType::Uuid => self.foreignUuid(&name),
            ModelKeyType::Ulid => self.foreignUlid(&name),
        };
        definition.for_model(model.getTable(), model.getKeyName())
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
    pub fn rawIndex(&mut self, expression: &str, name: &str) -> &mut IndexDefinition {
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

    pub fn fullText(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::FullText, columns, name)
    }

    pub fn spatialIndex(&mut self, columns: &[&str], name: Option<&str>) -> &mut IndexDefinition {
        self.add_index(IndexType::Spatial, columns, name)
    }

    pub fn vectorIndex(&mut self, column: &str, name: Option<&str>) -> &mut IndexDefinition {
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

    pub fn innoDb(&mut self) {
        self.engine("InnoDB");
    }

    pub fn comment(&mut self, comment: &str) {
        self.commands
            .push(Command::TableComment(comment.to_owned()));
    }

    pub fn temporary(&mut self) {
        self.temporary = true;
    }

    pub fn renameColumn(&mut self, from: &str, to: &str) {
        self.commands
            .push(Command::RenameColumn(from.into(), to.into()));
    }

    pub fn dropColumn(&mut self, columns: &[&str]) -> &mut DropColumnDefinition {
        self.commands
            .push(Command::DropColumn(DropColumnDefinition::new(columns)));
        let Some(Command::DropColumn(drop)) = self.commands.last_mut() else {
            unreachable!("drop column command was just appended")
        };
        drop
    }

    pub fn dropMorphs(&mut self, name: &str, index: Option<&str>) {
        let kind = format!("{name}_type");
        let id = format!("{name}_id");
        if let Some(index) = index {
            self.dropIndex(index);
        } else {
            self.dropIndexColumns(&[&kind, &id]);
        }
        self.dropColumn(&[&kind, &id]);
    }

    pub fn dropTimestamps(&mut self) {
        self.dropColumn(&["created_at", "updated_at"]);
    }

    pub fn dropTimestampsTz(&mut self) {
        self.dropTimestamps();
    }

    pub fn dropSoftDeletes(&mut self, column: Option<&str>) {
        self.dropColumn(&[column.unwrap_or("deleted_at")]);
    }

    pub fn dropSoftDeletesTz(&mut self, column: Option<&str>) {
        self.dropSoftDeletes(column);
    }

    pub fn dropRememberToken(&mut self) {
        self.dropColumn(&["remember_token"]);
    }

    pub fn dropConstrainedForeignId(&mut self, column: &str) {
        self.dropForeignColumns(&[column]);
        self.dropColumn(&[column]);
    }

    pub fn dropForeignIdFor<M: Model>(&mut self, column: Option<&str>) {
        let name = column.map_or_else(
            || M::from_state(ModelState::default()).getForeignKey(),
            str::to_owned,
        );
        self.dropColumn(&[&name]);
    }

    pub fn dropConstrainedForeignIdFor<M: Model>(&mut self, column: Option<&str>) {
        let name = column.map_or_else(
            || M::from_state(ModelState::default()).getForeignKey(),
            str::to_owned,
        );
        self.dropConstrainedForeignId(&name);
    }

    pub fn renameIndex(&mut self, from: &str, to: &str) {
        self.commands
            .push(Command::RenameIndex(from.into(), to.into()));
    }

    pub fn dropIndex(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Index, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn dropIndexColumns(&mut self, columns: &[&str]) {
        self.drop_index_columns(IndexType::Index, columns);
    }

    pub fn dropUnique(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Unique, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn dropUniqueColumns(&mut self, columns: &[&str]) {
        self.drop_index_columns(IndexType::Unique, columns);
    }

    pub fn dropPrimary(&mut self) {
        self.commands
            .push(Command::DropIndex(IndexType::Primary, String::new()));
    }

    pub fn dropFullText(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::FullText, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn dropFullTextColumns(&mut self, columns: &[&str]) {
        self.drop_index_columns(IndexType::FullText, columns);
    }

    pub fn dropSpatialIndex(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Spatial, name.into()));
    }

    /// Drops the index named by the table, ordered columns, and index kind.
    pub fn dropSpatialIndexColumns(&mut self, columns: &[&str]) {
        self.drop_index_columns(IndexType::Spatial, columns);
    }

    pub fn dropVectorIndex(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Vector, name.into()));
    }

    pub fn dropVectorIndexColumns(&mut self, columns: &[&str]) {
        self.drop_index_columns(IndexType::Vector, columns);
    }

    fn drop_index_columns(&mut self, kind: IndexType, columns: &[&str]) {
        self.commands.push(Command::DropIndexColumns(
            kind,
            columns.iter().map(|column| (*column).to_owned()).collect(),
        ));
    }

    pub fn dropForeignColumns(&mut self, columns: &[&str]) {
        self.commands.push(Command::DropForeignColumns(
            columns.iter().map(|column| (*column).to_owned()).collect(),
        ));
    }

    pub fn dropForeign(&mut self, name: &str) {
        self.commands.push(Command::DropForeign(name.into()));
    }
}
