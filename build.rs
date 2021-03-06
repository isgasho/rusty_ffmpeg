#![feature(bool_to_option)]
use bindgen::{self, callbacks, CargoCallbacks};
use once_cell::sync::{Lazy, OnceCell};
use pkg_config as pkgconfig;

use std::{collections::HashSet, convert::From, env, fs, path};

/// All the libs that FFmpeg has
static LIBS: Lazy<[&str; 8]> = Lazy::new(|| {
    [
        "avcodec",
        "avdevice",
        "avfilter",
        "avformat",
        "avutil",
        "postproc",
        "swresample",
        "swscale",
    ]
});

/// Whitelist of the headers we want to generate bindings
static HEADERS: Lazy<[&str; 64]> = Lazy::new(|| {
    [
        "libavcodec/avcodec.h",
        "libavcodec/avfft.h",
        "libavcodec/dv_profile.h",
        "libavcodec/vaapi.h",
        "libavcodec/vorbis_parser.h",
        "libavdevice/avdevice.h",
        "libavfilter/avfilter.h",
        "libavfilter/buffersink.h",
        "libavfilter/buffersrc.h",
        "libavformat/avformat.h",
        "libavformat/avio.h",
        "libavutil/adler32.h",
        "libavutil/aes.h",
        "libavutil/audio_fifo.h",
        "libavutil/avstring.h",
        "libavutil/avutil.h",
        "libavutil/base64.h",
        "libavutil/blowfish.h",
        "libavutil/bprint.h",
        "libavutil/buffer.h",
        "libavutil/camellia.h",
        "libavutil/cast5.h",
        "libavutil/channel_layout.h",
        "libavutil/cpu.h",
        "libavutil/crc.h",
        "libavutil/dict.h",
        "libavutil/display.h",
        "libavutil/downmix_info.h",
        "libavutil/error.h",
        "libavutil/eval.h",
        "libavutil/fifo.h",
        "libavutil/file.h",
        "libavutil/frame.h",
        "libavutil/hash.h",
        "libavutil/hmac.h",
        "libavutil/imgutils.h",
        "libavutil/lfg.h",
        "libavutil/log.h",
        "libavutil/macros.h",
        "libavutil/mathematics.h",
        "libavutil/md5.h",
        "libavutil/mem.h",
        "libavutil/motion_vector.h",
        "libavutil/murmur3.h",
        "libavutil/opt.h",
        "libavutil/parseutils.h",
        "libavutil/pixdesc.h",
        "libavutil/pixfmt.h",
        "libavutil/random_seed.h",
        "libavutil/rational.h",
        "libavutil/replaygain.h",
        "libavutil/ripemd.h",
        "libavutil/samplefmt.h",
        "libavutil/sha.h",
        "libavutil/sha512.h",
        "libavutil/stereo3d.h",
        "libavutil/threadmessage.h",
        "libavutil/time.h",
        "libavutil/timecode.h",
        "libavutil/twofish.h",
        "libavutil/xtea.h",
        "libpostproc/postprocess.h",
        "libswresample/swresample.h",
        "libswscale/swscale.h",
    ]
});

/// Filter out all symbols in the HashSet, and for others things it will act
/// exactly the same as `CargoCallback`.
#[derive(Debug)]
struct FilterCargoCallbacks(CargoCallbacks, HashSet<String>);

impl callbacks::ParseCallbacks for FilterCargoCallbacks {
    fn will_parse_macro(&self, _name: &str) -> callbacks::MacroParsingBehavior {
        if self.1.contains(_name) {
            callbacks::MacroParsingBehavior::Ignore
        } else {
            callbacks::MacroParsingBehavior::Default
        }
    }
    fn include_file(&self, _filename: &str) {
        self.0.include_file(_filename);
    }
}

fn out_dir() -> path::PathBuf {
    let x = OnceCell::new();
    x.get_or_init(|| path::PathBuf::from(env::var("OUT_DIR").unwrap()))
        .clone()
}

fn main() {
    // We currently only support building with static libraries.

    /* Thanks to pkg-config, we almost don't need this.
    // Output link libraries
    (&*LIBS)
        .iter()
        .for_each(|name| println!("cargo:rustc-link-lib={}={}", "static", name));
    */

    // Probe libraries
    // TODO if not enabled, we should not probe it
    let include_paths = (&*LIBS)
        .iter()
        .map(|name| "lib".to_owned() + name)
        .map(|libname| {
            pkgconfig::Config::new()
                .statik(true)
                .probe(&libname)
                .unwrap_or_else(|_| panic!(format!("{} not found!", libname)))
                .include_paths
        })
        .fold(HashSet::new(), |mut acc, paths| {
            paths.into_iter().for_each(|path| {
                acc.insert(path);
            });
            acc
        });

    // TODO mysterious feature checking should be done

    // Add clang path, for `#include` header finding
    let clang_args = include_paths
        .iter()
        .map(|path| "-I".to_owned() + path.to_str().unwrap());

    // Because the strange `FP_*` in `math.h` https://github.com/rust-lang/rust-bindgen/issues/687
    let filter_callback = FilterCargoCallbacks(
        CargoCallbacks,
        vec![
            "FP_NAN".to_owned(),
            "FP_INFINITE".to_owned(),
            "FP_ZERO".to_owned(),
            "FP_SUBNORMAL".to_owned(),
            "FP_NORMAL".to_owned(),
        ]
        .into_iter()
        .collect(),
    );

    // Bindgen the headers
    let builder = bindgen::builder()
        .clang_args(clang_args)
        .parse_callbacks(Box::new(filter_callback));

    let builder = (&*HEADERS)
        .iter()
        // map header short path to full path
        .map(|header| {
            include_paths
                .iter()
                .find_map(|path| {
                    let full_path = path.join(header);
                    println!("{}", full_path.to_string_lossy());
                    fs::metadata(&full_path).ok().map(|_| full_path)
                })
                .unwrap()
        })
        .fold(builder, |builder, header| {
            builder.header(header.to_str().unwrap())
        });

    /* This is a ill try, FFmpeg strangely generate corresponding header file
     * even if some feature is not usable. e.g. We get
     * libavcodec/videotoolbox.h(MacOS only) even when we are on Linux. SO
     * generate it's binding will fails. So I use the white list way.

    // Shrink to one path and search header in it. Using it will make the
    // situation that headers are not placed in one folder invalid.
    let include_path = if include_paths.len() > 1 {
        panic!("Inconsistent include paths");
    } else {
        include_paths.iter().next().expect("No include_path.")
    };

    // Find all headers in include path and generate binding to it.
    let builder = (&*LIBS)
        .iter()
        // TODO if not enabled, we should not manipulate it, consider adding a filter here
        .map(|name| "lib".to_owned() + name)
        .map(|libname| {
            let mut path = path::PathBuf::from(include_path);
            path.push(&libname);
            (libname, path.into_os_string())
        })
        .fold(
            // For each library(e.g. libavcodec), find all headers under its folder and `Builder::header()` it.
            bindgen::builder().parse_callbacks(Box::new(CargoCallbacks)),
            |builder, (libname, path)| {
                fs::read_dir(&path)
                    .expect(&format!("Cannot open libfolder:{}", libname))
                    .map(|entry| entry.unwrap())
                    // Filter out all entries which is file
                    .filter_map(|entry| entry.file_type().unwrap().is_file().then_some(entry))
                    // Filter out all files which name ends with `.h`
                    .filter_map(|entry| {
                        let name = entry.file_name();
                        name.to_string_lossy().ends_with(".h").then_some(name)
                    })
                    // Builder binds header files
                    .fold(builder, |builder, name| {
                        let file_path: path::PathBuf = [path.clone(), name].iter().collect();
                        builder.header(file_path.to_str().expect("invalid Unicode header name!"))
                    })
            },
        );
    */

    // Is it correct to generate binding to one file? :-/
    let output_path: path::PathBuf = [out_dir(), "binding.rs".into()].iter().collect();

    builder
        .generate()
        .expect("Binding generation failed.")
        .write_to_file(output_path)
        .expect("Cannot write binding to file!")
}
