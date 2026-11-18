use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

struct AppRoot(PathBuf);

impl AppRoot {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openrust-mail-generator-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn artisan(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &self.0)
            .output()
            .unwrap()
    }

    fn run(&self, args: &[&str]) {
        let result = self.artisan(args);
        assert!(result.status.success(), "{args:?}: {result:?}");
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.0.join(relative)).unwrap()
    }
}

impl Drop for AppRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn artisan_generates_mailables_and_named_templates() {
    let root = AppRoot::new();
    let help = root.artisan(&["make:mail", "--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    for option in ["--force", "--markdown", "--view", "--test"] {
        assert!(help.contains(option), "{help}");
    }

    root.run(&["make:mail", "DeliveryNotice"]);
    assert!(
        root.read("app/mail/delivery_notice.rs")
            .contains("text: Some(\"Delivery Notice\"")
    );
    assert!(!root.0.join("resources/views").exists());

    root.run(&["make:mail", "Orders/Shipped", "--markdown", "--test"]);
    assert!(
        root.read("app/mail/orders/shipped.rs")
            .contains("markdown: Some(\"mail.orders.shipped\"")
    );
    assert!(
        root.read("resources/views/mail/orders/shipped.blade.html")
            .contains("<x-mail::message>")
    );
    assert!(
        root.0
            .join("tests/feature/mail/orders/shipped_test.rs")
            .is_file()
    );

    root.run(&[
        "make:mail",
        "App\\Mail\\BuyerReceipt",
        "--view=receipts.buyer",
    ]);
    assert!(
        root.read("app/mail/buyer_receipt.rs")
            .contains("view: Some(\"receipts.buyer\"")
    );
    assert!(
        root.read("resources/views/receipts/buyer.blade.html")
            .contains("<p>")
    );
    root.run(&[
        "make:mail",
        "ManualReceipt",
        "--view=legacy",
        "--markdown=receipts.manual",
    ]);
    assert!(
        root.read("app/mail/manual_receipt.rs")
            .contains("markdown: Some(\"receipts.manual\"")
    );
    assert!(!root.0.join("resources/views/legacy.blade.html").exists());
}

#[test]
fn artisan_respects_published_overrides_and_keeps_existing_mail_templates() {
    let root = AppRoot::new();
    root.run(&["stub:publish"]);
    for name in ["mail", "view-mail", "markdown-mail"] {
        assert!(
            root.read(&format!("stubs/{name}.rs.stub"))
                .contains("{{class}}")
        );
    }
    let stub = "stubs/markdown-mail.rs.stub";
    fs::write(
        root.0.join(stub),
        format!("// project override\n{}", root.read(stub)),
    )
    .unwrap();
    let args = ["make:mail", "InvoiceReady", "-m"];
    root.run(&args);
    assert!(
        root.read("app/mail/invoice_ready.rs")
            .starts_with("// project override")
    );
    fs::write(
        root.0.join("app/mail/invoice_ready.rs"),
        "// existing source",
    )
    .unwrap();
    fs::write(
        root.0.join("resources/views/mail/invoice-ready.blade.html"),
        "Existing template",
    )
    .unwrap();

    let duplicate = root.artisan(&args);
    assert!(duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already exists"));
    assert_eq!(root.read("app/mail/invoice_ready.rs"), "// existing source");

    root.run(&["make:mail", "InvoiceReady", "-m", "-f"]);
    assert!(
        root.read("app/mail/invoice_ready.rs")
            .starts_with("// project override")
    );
    assert_eq!(
        root.read("resources/views/mail/invoice-ready.blade.html"),
        "Existing template"
    );
}

#[test]
fn artisan_validates_source_and_view_paths_before_writing() {
    let root = AppRoot::new();
    for args in [
        ["make:mail", "Mail/../Escaped", "--view"],
        ["make:mail", "Report", "--markdown=mail..outside"],
    ] {
        assert!(!root.artisan(&args).status.success());
    }
    assert!(!root.0.join("app").exists());
    assert!(!root.0.join("resources").exists());

    let blocked_view = root.0.join("resources/views/mail/report.blade.html");
    fs::create_dir_all(&blocked_view).unwrap();
    assert!(
        !root
            .artisan(&["make:mail", "Report", "--view"])
            .status
            .success()
    );
    assert!(!root.0.join("app/mail/report.rs").exists());

    #[cfg(unix)]
    {
        let escape = AppRoot::new();
        let outside = AppRoot::new();
        fs::create_dir(escape.0.join("app")).unwrap();
        std::os::unix::fs::symlink(&outside.0, escape.0.join("app/mail")).unwrap();
        assert!(
            !escape
                .artisan(&["make:mail", "Report", "--force"])
                .status
                .success()
        );
        assert!(!outside.0.join("report.rs").exists());
    }
}
