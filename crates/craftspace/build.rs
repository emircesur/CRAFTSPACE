//! Embed the icon and version information in the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/craftspace.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/craftspace.ico");
        res.set("FileDescription", "CraftSpace");
        res.set("ProductName", "CraftSpace");
        res.set("CompanyName", "CraftSpace contributors");
        if let Err(err) = res.compile() {
            // A missing resource compiler shouldn't stop development builds.
            println!("cargo:warning=could not embed the Windows icon: {err}");
        }
    }
}
