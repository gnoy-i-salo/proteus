use std::{env, path::PathBuf};

#[path = "src/e5-tokenizer-pin.rs"]
mod tokenizer_package;

fn main() {
    println!("cargo:rerun-if-changed=../../../scripts/e5-package.json");
    let tokenizer_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/models/e5-tokenizer.json");
    println!("cargo:rerun-if-changed={}", tokenizer_path.display());
    let pin = tokenizer_package::pin().expect("valid tokenizer package pin");
    tokenizer_package::read(&tokenizer_path, &pin)
        .expect("Run pnpm prepare:models to obtain the verified tokenizer");
    println!("cargo:warning=Verified E5 tokenizer {}", pin.revision);
    tauri_build::build();
    if env::var_os("CARGO_FEATURE_TELEGRAM").is_some() {
        build_tdlib();
    }
}

fn build_tdlib() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.cache/td-4c6377aa7386c39217878a9a56d581922ad37ca4");
    assert!(
        source.join(".proteus-verified").is_file(),
        "Run pnpm prepare:tdlib before building Telegram support"
    );
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PROTEUS_ZLIB_ROOT");
    let target = env::var("TARGET").expect("target");
    let windows = target.contains("windows");
    let apple = target.contains("apple");
    let openssl = openssl_src::Build::new().build();
    let library = |name: &str| {
        openssl.lib_dir().join(if windows {
            format!("lib{name}.lib")
        } else {
            format!("lib{name}.a")
        })
    };
    let mut config = cmake::Config::new(&source);
    config
        .profile("Release")
        .define("TD_INSTALL_STATIC_LIBRARIES", "ON")
        .define("TD_INSTALL_SHARED_LIBRARIES", "OFF")
        .define(
            "OPENSSL_ROOT_DIR",
            openssl.lib_dir().parent().expect("OpenSSL root"),
        )
        .define("OPENSSL_INCLUDE_DIR", openssl.include_dir())
        .define("OPENSSL_CRYPTO_LIBRARY", library("crypto"))
        .define("OPENSSL_SSL_LIBRARY", library("ssl"))
        .define("OPENSSL_USE_STATIC_LIBS", "TRUE")
        .define("TD_ENABLE_JNI", "OFF")
        .define("TD_ENABLE_LTO", "OFF")
        .build_target("tdjson_static");
    let zlib_link_name = env::var_os("PROTEUS_ZLIB_ROOT").map(|zlib| {
        let zlib = PathBuf::from(zlib);
        let name = ["zs", "zlib"]
            .into_iter()
            .find(|name| zlib.join(format!("lib/{name}.lib")).is_file())
            .expect("PROTEUS_ZLIB_ROOT/lib must contain zs.lib or zlib.lib");
        config
            .define("ZLIB_ROOT", &zlib)
            .define("ZLIB_INCLUDE_DIR", zlib.join("include"))
            .define("ZLIB_LIBRARY", zlib.join(format!("lib/{name}.lib")));
        println!(
            "cargo:rustc-link-search=native={}",
            zlib.join("lib").display()
        );
        name
    });
    // macOS Homebrew may contain Intel-only zlib on an ARM host. Use the SDK library.
    if apple {
        let output = std::process::Command::new("xcrun")
            .arg("--show-sdk-path")
            .output()
            .expect("macOS SDK");
        assert!(output.status.success(), "xcrun failed");
        let sdk = PathBuf::from(String::from_utf8(output.stdout).expect("SDK path").trim());
        config
            .define("ZLIB_INCLUDE_DIR", sdk.join("usr/include"))
            .define("ZLIB_LIBRARY", sdk.join("usr/lib/libz.tbd"));
    }
    let destination = config.build();
    let build = destination.join("build");
    for directory in ["", "tdutils", "tdactor", "tdnet", "tddb", "tde2e", "sqlite"] {
        let path = build.join(directory);
        println!("cargo:rustc-link-search=native={}", path.display());
        if windows {
            println!(
                "cargo:rustc-link-search=native={}",
                path.join("Release").display()
            );
        }
    }
    let mut libraries = vec!["tdjson_static", "tdjson_private", "tdclient", "tdcore"];
    libraries.extend([
        "tdapi",
        "tdmtproto",
        "tdnet",
        "tddb",
        "tdactor",
        "tde2e",
        "tdsqlite",
        "tdutils",
    ]);
    for library in libraries {
        println!("cargo:rustc-link-lib=static={library}");
    }
    openssl.print_cargo_metadata();
    if windows {
        let zlib = zlib_link_name.expect("Set PROTEUS_ZLIB_ROOT to the static zlib installation");
        println!("cargo:rustc-link-lib=static={zlib}");
        for library in [
            "ws2_32", "mswsock", "crypt32", "normaliz", "psapi", "shell32",
        ] {
            println!("cargo:rustc-link-lib={library}");
        }
    } else {
        println!("cargo:rustc-link-lib=z");
        println!(
            "cargo:rustc-link-lib={}",
            if apple { "c++" } else { "stdc++" }
        );
        if apple {
            println!("cargo:rustc-link-lib=framework=Foundation");
        } else {
            println!("cargo:rustc-link-lib=dl");
        }
    }
}
