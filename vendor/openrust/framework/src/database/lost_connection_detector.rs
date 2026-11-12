use std::{error::Error, io::ErrorKind};

use crate::contracts::database::LostConnectionDetector as Detector;

/// Recognizes database and Redis transport failures using driver error types.
pub struct LostConnectionDetector;

impl Detector for LostConnectionDetector {
    fn causedByLostConnection(&self, error: &(dyn Error + 'static)) -> bool {
        let mut source = Some(error);
        while let Some(error) = source {
            if error
                .downcast_ref::<sqlx::Error>()
                .is_some_and(sql_connection_lost)
                || error
                    .downcast_ref::<::redis::RedisError>()
                    .is_some_and(::redis::RedisError::is_connection_dropped)
            {
                return true;
            }
            source = error.source();
        }
        false
    }
}

fn sql_connection_lost(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(error) => matches!(
            error.kind(),
            ErrorKind::BrokenPipe
                | ErrorKind::ConnectionAborted
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionReset
                | ErrorKind::NetworkUnreachable
                | ErrorKind::NotConnected
                | ErrorKind::TimedOut
                | ErrorKind::UnexpectedEof
        ),
        sqlx::Error::Database(error) => {
            error.code().is_some_and(|code| {
                code.starts_with("08") || matches!(code.as_ref(), "57P01" | "57P02" | "57P03")
            }) || error
                .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                .is_some_and(|error| matches!(error.number(), 1053 | 1927 | 2006 | 2013))
        }
        _ => false,
    }
}
