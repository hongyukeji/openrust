use openrust::database::{
    Value,
    eloquent::{
        Attributes,
        factories::{Factory, HasFactory},
    },
};

use crate::app::models::User;

pub(crate) struct UserFactory;

impl UserFactory {
    pub(crate) fn factory() -> Factory<User> {
        Factory::tryWithFaker(|faker| {
            let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
            Ok(Attributes::from([
                ("name".into(), faker.name().into()),
                ("email".into(), faker.unique(false).safeEmail()?.into()),
                ("email_verified_at".into(), now.to_rfc3339().into()),
                ("password".into(), "password".into()),
                (
                    "remember_token".into(),
                    faker.regexify("[A-Za-z0-9]{10}")?.into(),
                ),
            ]))
        })
    }

    #[allow(
        dead_code,
        reason = "Application factory state available to tests and seeders"
    )]
    pub(crate) fn unverified() -> Factory<User> {
        Self::factory().set("email_verified_at", Value::Null)
    }
}

impl HasFactory for User {
    fn factory() -> Factory<Self> {
        UserFactory::factory()
    }
}
