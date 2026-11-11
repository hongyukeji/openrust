use super::{
    ColumnDefinition, ForeignIdColumnDefinition, ForeignKeyDefinition, IndexDefinition,
    column_definition::ColumnType, index_definition::IndexType,
};

pub(super) enum Command {
    Column(ColumnDefinition),
    Index(IndexDefinition),
    Foreign(ForeignKeyDefinition),
    RenameColumn(String, String),
    DropColumn(String),
    RenameIndex(String, String),
    DropIndex(IndexType, String),
    DropForeign(String),
    DropForeignColumns(Vec<String>),
}

/// A table definition or a sequence of changes to an existing table.
#[derive(Default)]
pub struct Blueprint {
    pub(super) commands: Vec<Command>,
    pub(super) temporary: bool,
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
        self.commands
            .push(Command::Column(ColumnDefinition::new(name, kind)));
        let Some(Command::Column(column)) = self.commands.last_mut() else {
            unreachable!("column was just appended")
        };
        column
    }

    column_types! {
        (tinyInteger, TinyInteger), (smallInteger, SmallInteger),
        (mediumInteger, MediumInteger), (integer, Integer), (bigInteger, BigInteger),
        (text, Text), (mediumText, MediumText), (longText, LongText), (boolean, Boolean),
        (float, Float), (double, Double), (binary, Binary), (json, Json), (jsonb, Json),
        (date, Date),
    }

    pub fn id(&mut self) -> &mut ColumnDefinition {
        self.bigIncrements("id")
    }

    pub fn increments(&mut self, name: &str) -> &mut ColumnDefinition {
        self.integer(name).autoIncrement()
    }

    pub fn bigIncrements(&mut self, name: &str) -> &mut ColumnDefinition {
        self.bigInteger(name).autoIncrement()
    }

    pub fn unsignedBigInteger(&mut self, name: &str) -> &mut ColumnDefinition {
        self.bigInteger(name).unsigned()
    }

    pub fn unsignedInteger(&mut self, name: &str) -> &mut ColumnDefinition {
        self.integer(name).unsigned()
    }

    pub fn string(&mut self, name: &str, length: u32) -> &mut ColumnDefinition {
        self.column(name, ColumnType::String(length))
    }

    pub fn char(&mut self, name: &str, length: u32) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Char(length))
    }

    pub fn uuid(&mut self, name: &str) -> &mut ColumnDefinition {
        self.char(name, 36)
    }

    pub fn ulid(&mut self, name: &str) -> &mut ColumnDefinition {
        self.char(name, 26)
    }

    pub fn decimal(&mut self, name: &str, total: u8, places: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Decimal(total, places))
    }

    pub fn timestamp(&mut self, name: &str, precision: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Timestamp(precision))
    }

    pub fn dateTime(&mut self, name: &str, precision: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::DateTime(precision))
    }

    pub fn time(&mut self, name: &str, precision: u8) -> &mut ColumnDefinition {
        self.column(name, ColumnType::Time(precision))
    }

    pub fn timestamps(&mut self) {
        self.timestamp("created_at", 0).nullable();
        self.timestamp("updated_at", 0).nullable();
    }

    pub fn softDeletes(&mut self) -> &mut ColumnDefinition {
        self.timestamp("deleted_at", 0).nullable()
    }

    pub fn foreignId(&mut self, name: &str) -> ForeignIdColumnDefinition<'_> {
        self.unsignedBigInteger(name);
        ForeignIdColumnDefinition::new(self, name)
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

    pub fn temporary(&mut self) {
        self.temporary = true;
    }

    pub fn renameColumn(&mut self, from: &str, to: &str) {
        self.commands
            .push(Command::RenameColumn(from.into(), to.into()));
    }

    pub fn dropColumn(&mut self, columns: &[&str]) {
        self.commands.extend(
            columns
                .iter()
                .map(|column| Command::DropColumn((*column).into())),
        );
    }

    pub fn renameIndex(&mut self, from: &str, to: &str) {
        self.commands
            .push(Command::RenameIndex(from.into(), to.into()));
    }

    pub fn dropIndex(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Index, name.into()));
    }

    pub fn dropUnique(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Unique, name.into()));
    }

    pub fn dropPrimary(&mut self) {
        self.commands
            .push(Command::DropIndex(IndexType::Primary, String::new()));
    }

    pub fn dropFullText(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::FullText, name.into()));
    }

    pub fn dropSpatialIndex(&mut self, name: &str) {
        self.commands
            .push(Command::DropIndex(IndexType::Spatial, name.into()));
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
