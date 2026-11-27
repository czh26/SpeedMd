fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "ThunderMD")
            .set("FileDescription", "ThunderMD");
        res.compile().expect("failed to compile Windows icon resources");
    }
}
