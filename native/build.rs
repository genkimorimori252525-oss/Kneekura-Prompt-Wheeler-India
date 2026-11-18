// Embeds the application icon and version info into the .exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "Love愛ドーナツ");
        resource.set("FileDescription", "Love愛ドーナツ");
        resource.compile().expect("embed icon");
    }
}
