use std::{env, path::PathBuf};

fn main() {
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
        .build_target("tdjson_static");
    if let Some(zlib) = env::var_os("PROTEUS_ZLIB_ROOT") {
        let zlib = PathBuf::from(zlib);
        config
            .define("ZLIB_ROOT", &zlib)
            .define("ZLIB_INCLUDE_DIR", zlib.join("include"))
            .define("ZLIB_LIBRARY", zlib.join("lib/zlib.lib"));
        println!(
            "cargo:rustc-link-search=native={}",
            zlib.join("lib").display()
        );
    }
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
    let mut libraries = vec!["tdjson_static", "tdjson_private", "tdclient"];
    if windows {
        libraries.extend(["tdcore_part1", "tdcore_part2"]);
    } else {
        libraries.push("tdcore");
    }
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
        println!("cargo:rustc-link-lib=static=zlib");
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
