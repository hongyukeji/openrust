fn main() {
    // 原生验收程序使用与应用入口相同的 Windows 栈预留。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo::rustc-link-arg-examples=/STACK:16777216");
    }
}
