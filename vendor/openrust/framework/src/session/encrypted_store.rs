use std::{sync::Arc, time::Duration};

use crate::contracts::{encryption::Encrypter, session::SessionHandler};

use super::{SessionError, SessionFuture, SessionResult};

/// Encrypts the serialized session at the storage boundary.
pub struct EncryptedStore {
    handler: Box<dyn SessionHandler>,
    encrypter: Arc<dyn Encrypter>,
}

impl EncryptedStore {
    pub fn new(handler: Box<dyn SessionHandler>, encrypter: Arc<dyn Encrypter>) -> Self {
        Self { handler, encrypter }
    }
}

impl SessionHandler for EncryptedStore {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            Ok(self
                .handler
                .read(id)
                .await?
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|encrypted| self.encrypter.decrypt(&encrypted, true).ok()))
        })
    }

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        payload: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            let encrypted = self
                .encrypter
                .encrypt(payload, true)
                .map_err(|_| SessionError::Encoding)?;
            self.handler.write(id, encrypted.as_bytes()).await
        })
    }

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        self.handler.destroy(id)
    }

    fn gc(&mut self, lifetime: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        self.handler.gc(lifetime)
    }
}
