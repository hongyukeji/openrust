use std::path::{Path, PathBuf};

use super::{PgConnectOptions, PgSslMode, SchemaCredentials, dump_command, restore_command};

#[test]
fn postgres_snapshot_clients_preserve_the_selected_endpoint_and_stop_on_restore_errors() {
    let options = PgConnectOptions::new()
        .host("writer.test")
        .port(6432)
        .username("schema-owner")
        .database("db'\\name")
        .ssl_mode(PgSslMode::VerifyFull);
    let credentials = SchemaCredentials {
        password: "synthetic password".into(),
        ssl_ca: Some(PathBuf::from("database/ca.pem")),
    };
    let export = dump_command(&options, &credentials).unwrap();
    let native = export.as_std();
    assert_eq!(native.get_program(), "pg_dump");
    assert_eq!(
        native.get_args().collect::<Vec<_>>(),
        [
            "--no-password",
            "--dbname",
            "host='writer.test' port=6432 user='schema-owner' dbname='db\\'\\\\name' sslmode=verify-full",
            "--no-owner",
            "--no-acl",
        ]
    );
    let env: std::collections::BTreeMap<_, _> = native.get_envs().collect();
    assert_eq!(
        env[std::ffi::OsStr::new("PGPASSWORD")],
        Some("synthetic password".as_ref())
    );
    assert_eq!(
        env[std::ffi::OsStr::new("PGSSLROOTCERT")],
        Some("database/ca.pem".as_ref())
    );
    assert_eq!(env[std::ffi::OsStr::new("PGHOSTADDR")], None);

    let (program, restore) = restore_command(&options, &credentials, Path::new("db.sql")).unwrap();
    assert_eq!(program, "psql");
    let arguments: Vec<_> = restore.as_std().get_args().collect();
    assert!(arguments.ends_with(&[
        "--no-psqlrc".as_ref(),
        "--set=ON_ERROR_STOP=1".as_ref(),
        "--single-transaction".as_ref(),
        "--file=-".as_ref(),
    ]));
    let (program, restore) = restore_command(&options, &credentials, Path::new("db.dump")).unwrap();
    assert_eq!(program, "pg_restore");
    let arguments: Vec<_> = restore.as_std().get_args().collect();
    assert!(arguments.ends_with(&[
        "--no-owner".as_ref(),
        "--no-acl".as_ref(),
        "--clean".as_ref(),
        "--if-exists".as_ref(),
        "--single-transaction".as_ref(),
    ]));
}
