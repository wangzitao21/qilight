fn main() {
    println!("cargo:rerun-if-changed=assets/lamp.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/lamp.ico")
            .set("ProductName", "QiLight")
            .set("FileDescription", env!("CARGO_PKG_DESCRIPTION"))
            .set("FileVersion", env!("CARGO_PKG_VERSION"))
            .set("ProductVersion", env!("CARGO_PKG_VERSION"))
            .set("OriginalFilename", "QiLight.exe")
            .compile()
            .expect("无法编译 Windows 图标资源，请检查 windres 或 Windows SDK");
    }
}
