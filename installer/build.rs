use base64::Engine;
use std::{env, fs, path::Path};

fn mime(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

// Builds a page with the shared styles and every picture inlined.
fn page(file: &str, out: &str) {
    let base = fs::read_to_string("ui/base.css").expect("ui/base.css");
    let mut html = fs::read_to_string(Path::new("ui").join(file))
        .expect(file)
        .replace("/* base.css */", &base);
    let mut map = Vec::new();
    let mut names: Vec<_> = fs::read_dir("ui/assets")
        .expect("ui/assets")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    for name in names {
        let bytes = fs::read(Path::new("ui/assets").join(&name)).unwrap();
        let url = format!(
            "data:{};base64,{}",
            mime(&name),
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        html = html.replace(&format!("assets/{name}"), &url);
        map.push(format!("{:?}:{:?}", name, url));
    }
    // The page declares `const ASSET = {};`; fill it in with every embedded file, keyed by name.
    html = html.replace(
        "const ASSET = {};",
        &format!("const ASSET = {{{}}};", map.join(",")),
    );
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join(out), html).unwrap();
}

fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-env-changed=MURMINT_PAYLOAD");
    println!("cargo:rerun-if-env-changed=MURMINT_UNINSTALLER");
    page("index.html", "setup.html");
    page("uninstall.html", "uninstall.html");

    if env::var("MURMINT_PAYLOAD").is_err() {
        panic!("set MURMINT_PAYLOAD to the regular installer to wrap (scripts/build-installer.js does this)");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env::var("MURMINT_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../build/icon.ico")
            .set("ProductName", "Murmint")
            .set("FileDescription", "Murmint Setup")
            .set("CompanyName", "Murmint")
            .set("LegalCopyright", "Murmint")
            .set("ProductVersion", &version)
            .set("FileVersion", &version);
        res.compile().expect("resource");
    }
}
