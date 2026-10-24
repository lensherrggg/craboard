fn main() {
    println!("cargo:rerun-if-changed=assets/craboard.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/craboard.ico");
    res.set("ProductName", "Craboard");
    res.set("FileDescription", "Craboard");
    if let Err(e) = res.compile() {
        panic!("编译 Windows 图标资源失败：{e}");
    }
}
