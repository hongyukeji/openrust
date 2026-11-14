use openrust::database::{
    DatabaseFuture, DatabaseResult,
    eloquent::{Attributes, factories::HasFactory},
    seeder::{Seeder, SeederContext},
};

use crate::app::models::User;

pub struct DatabaseSeeder;

impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            User::factory()
                .createOneOn(
                    context.connection(),
                    Attributes::from([
                        ("name".into(), "Test User".into()),
                        ("email".into(), "test@example.com".into()),
                    ]),
                )
                .await?;
            Ok(())
        })
    }
}
