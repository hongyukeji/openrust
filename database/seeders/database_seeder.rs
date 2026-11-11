use openrust::database::{
    DatabaseFuture, DatabaseResult,
    seeder::{Seeder, SeederContext},
};

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        _context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
