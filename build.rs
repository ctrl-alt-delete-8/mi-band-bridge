fn main() {
    println!("cargo:rerun-if-changed=vendor/tiny-ecdh/ecdh.c");
    println!("cargo:rerun-if-changed=vendor/tiny-ecdh/ecdh.h");

    cc::Build::new()
        .file("vendor/tiny-ecdh/ecdh.c")
        .include("vendor/tiny-ecdh")
        .define("ECC_CURVE", "1") // NIST_B163
        .opt_level(2)
        .warnings(false)
        .compile("tinyecdh");

    // macOS decides Bluetooth access from the responsible process. Under a
    // shell pipeline the terminal's grant does not apply, and TCC kills the
    // process with no message. Embedding the usage description in a Mach-O
    // section makes the binary self-describing, with no .app bundle.
    //
    // Only a Mac binary has the section. Windows asks no such question, and
    // its linker does not know the argument.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this");
    let plist = std::path::Path::new(&manifest_dir).join("Info.plist");
    println!("cargo:rerun-if-changed=Info.plist");
    println!(
        "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        plist.display()
    );
}
