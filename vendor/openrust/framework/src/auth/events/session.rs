use std::sync::Arc;

use crate::{
    auth::{AuthIdentifier, Credentials},
    contracts::{auth::Authenticatable, events::Event},
    events::EventContext,
};

/// A session guard is about to check credentials.
pub struct Attempting {
    pub guard: String,
    pub remember: bool,
}

/// A provider accepted the supplied credentials.
pub struct Validated {
    pub guard: String,
    pub user: AuthIdentifier,
}

impl Validated {
    /// Borrows the candidate user before any password rehash.
    pub fn user<'a, T: Authenticatable>(&self, context: &EventContext<'a>) -> Option<&'a T> {
        context.arguments_for(self)?.first()?.downcast_ref()
    }
}

/// A guard persisted an authenticated session, including remembered logins.
pub struct Login {
    pub guard: String,
    pub user: Arc<dyn Authenticatable>,
    pub remember: bool,
}

/// A guard resolved or explicitly installed an authenticated user.
pub struct Authenticated {
    pub guard: String,
    pub user: Arc<dyn Authenticatable>,
}

/// A credential attempt was rejected.
pub struct Failed {
    pub guard: String,
    pub user: Option<Arc<dyn Authenticatable>>,
}

/// The guard ended the session and invalidated the remember token.
pub struct Logout {
    pub guard: String,
    pub user: Option<Arc<dyn Authenticatable>>,
}

/// The guard ended authentication on the current device only.
pub struct CurrentDeviceLogout {
    pub guard: String,
    pub user: Option<Arc<dyn Authenticatable>>,
}

macro_rules! event_names {
    ($($event:ident),+ $(,)?) => {
        $(impl Event for $event {
            fn name(&self) -> &str {
                stringify!($event)
            }
        })+
    };
}

event_names!(
    Attempting,
    Validated,
    Login,
    Authenticated,
    Failed,
    Logout,
    CurrentDeviceLogout
);

macro_rules! borrowed_credentials {
    ($($event:ident),+ $(,)?) => {
        $(impl $event {
            /// Borrows the credentials during synchronous listener delivery.
            pub fn credentials<'a>(&self, context: &EventContext<'a>) -> Option<&'a Credentials> {
                context.arguments_for(self)?.first()?.downcast_ref()
            }
        })+
    };
}

borrowed_credentials!(Attempting, Failed);
