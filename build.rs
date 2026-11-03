fn main() {
    println!("cargo:rerun-if-changed=native/speech.c");
    println!("cargo:rerun-if-changed=vendor/sonic/sonic.c");
    println!("cargo:rerun-if-changed=vendor/sonic/sonic.h");
    println!("cargo:rerun-if-changed=native/metrics.c");
    println!("cargo:rerun-if-changed=native/exceptions.cpp");
    println!("cargo:rerun-if-changed=native/macos.m");
    println!("cargo:rerun-if-changed=vendor/sherpa-onnx/c-api.h");
    cc::Build::new()
        .file("native/speech.c")
        .file("native/metrics.c")
        .include("vendor/sherpa-onnx")
        .flag_if_supported("-std=c11")
        .compile("infotransfer_speech");
    cc::Build::new()
        .file("vendor/sonic/sonic.c")
        .warnings(false)
        .compile("infotransfer_tempo");
    cc::Build::new()
        .cpp(true)
        .file("native/exceptions.cpp")
        .include("vendor/sherpa-onnx")
        .flag_if_supported("-std=c++17")
        .compile("infotransfer_exceptions");
    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .compile("infotransfer_macos");
        for framework in [
            "AppKit",
            "ApplicationServices",
            "Carbon",
            "ServiceManagement",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
