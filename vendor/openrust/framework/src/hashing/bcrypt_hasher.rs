use super::{
    BcryptConfig,
    driver::{Settings, define_hasher},
};

define_hasher!(
    BcryptHasher,
    BcryptConfig,
    Settings::Bcrypt,
    "Hashes passwords using bcrypt."
);
