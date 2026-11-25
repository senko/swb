//! Build script of swb-media-ffi (ADR 0047 sections 1 and 4).
//!
//! Without the feature `ffmpeg` it does nothing. With it, it checks that
//! the FFmpeg build of `just ffmpeg` (or `SWB_FFMPEG_DIR`) is the pinned
//! one (`tools/ffmpeg/`), compiles `csrc/ffmpeg.c` with warnings as errors,
//! and links FFmpeg's static libraries. It never downloads or builds
//! FFmpeg. `SWB_MEDIA_ASAN=1` compiles the C file with AddressSanitizer
//! and UndefinedBehaviorSanitizer (`just media-asan`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "ffmpeg")]
    ffmpeg::build();
}

#[cfg(feature = "ffmpeg")]
mod ffmpeg {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// The static libraries, in link order.
    const LIBRARIES: [&str; 5] = ["avformat", "avcodec", "swscale", "swresample", "avutil"];

    /// Stops the build with one message.
    fn fail(message: &str) -> ! {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    fn read(path: &Path) -> String {
        println!("cargo:rerun-if-changed={}", path.display());
        fs::read_to_string(path)
            .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", path.display())))
    }

    /// The value of `KEY=VALUE` in `tools/ffmpeg/ffmpeg.env`.
    fn pin<'a>(pins: &'a str, key: &str) -> &'a str {
        pins.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| fail(&format!("tools/ffmpeg/ffmpeg.env has no {key}")))
    }

    /// The options of `tools/ffmpeg/configure.txt`, joined by spaces.
    fn options(text: &str) -> String {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(super) fn build() {
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
        let root = manifest.join("../..");
        let pins = read(&root.join("tools/ffmpeg/ffmpeg.env"));
        let version = pin(&pins, "FFMPEG_VERSION");
        let sha256 = pin(&pins, "FFMPEG_SHA256");
        let configure = options(&read(&root.join("tools/ffmpeg/configure.txt")));

        println!("cargo:rerun-if-env-changed=SWB_FFMPEG_DIR");
        println!("cargo:rerun-if-env-changed=SWB_MEDIA_ASAN");
        println!("cargo:rerun-if-changed=csrc/ffmpeg.c");
        println!("cargo:rerun-if-changed=csrc/ffmpeg.h");
        let prefix = env::var_os("SWB_FFMPEG_DIR")
            .map_or_else(|| root.join("out/ffmpeg").join(version), PathBuf::from);

        let not_built = || -> ! {
            fail(&format!(
                "FFmpeg {version} is not built (or was built with other options): run `just ffmpeg` \
                 (ADR 0047), or set SWB_FFMPEG_DIR (looked in {})",
                prefix.display()
            ))
        };
        let stamp_path = prefix.join("swb-build.txt");
        println!("cargo:rerun-if-changed={}", stamp_path.display());
        let Ok(stamp) = fs::read_to_string(&stamp_path) else {
            not_built()
        };
        let has = |line: String| stamp.lines().any(|l| l == line);
        if !has(format!("version={version}"))
            || !has(format!("sha256={sha256}"))
            || !has(format!("configure={configure}"))
        {
            not_built();
        }
        let lib = prefix.join("lib");
        if LIBRARIES
            .iter()
            .any(|name| !lib.join(format!("lib{name}.a")).is_file())
        {
            not_built();
        }

        let mut c = cc::Build::new();
        c.file("csrc/ffmpeg.c")
            .include(prefix.join("include"))
            .std("c11")
            .warnings(true)
            .extra_warnings(true)
            .warnings_into_errors(true)
            .flag("-Wno-unused-parameter");
        if env::var_os("SWB_MEDIA_ASAN").is_some_and(|v| v == "1") {
            // The C boundary only; FFmpeg itself is not instrumented. Rust
            // links the runtimes late: run with
            // ASAN_OPTIONS=verify_asan_link_order=0.
            c.flag("-fsanitize=address,undefined")
                .flag("-fno-sanitize-recover=undefined")
                .flag("-fno-omit-frame-pointer");
            println!("cargo:rustc-link-lib=dylib=asan");
            println!("cargo:rustc-link-lib=dylib=ubsan");
        }
        c.compile("swb_ffmpeg");

        println!("cargo:rustc-link-search=native={}", lib.display());
        for name in LIBRARIES {
            println!("cargo:rustc-link-lib=static={name}");
        }
        // FFmpeg's pkg-config files: -lm -latomic -pthread.
        for name in ["m", "atomic", "pthread"] {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
        println!("cargo:rustc-env=SWB_FFMPEG_VERSION={version}");
        println!("cargo:rustc-env=SWB_FFMPEG_SHA256={sha256}");
        println!("cargo:rustc-env=SWB_FFMPEG_CONFIGURE={configure}");
    }
}
