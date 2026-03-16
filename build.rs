use std::env;
use std::path::{Path, PathBuf};

fn main() {
    #[cfg(feature = "nam")]
    {
        let nam_core_dir = PathBuf::from("NeuralAmpModelerCore");
        let nam_src_dir = nam_core_dir.join("NAM");
        let deps_dir = nam_core_dir.join("Dependencies");
        let eigen_dir = deps_dir.join("eigen");
        let nlohmann_dir = deps_dir.join("nlohmann");

        // Gather all cpp files in NeuralAmpModelerCore/NAM
        let mut nam_sources = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&nam_src_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("cpp") {
                    nam_sources.push(path);
                }
            }
        }

        // Combine NAM sources and our wrapper
        let mut build = cxx_build::bridge("src/nam.rs");

        build.file("src/cpp/nam_wrapper.cpp").includes(&[
            env::current_dir().unwrap().join("src"),
            env::current_dir().unwrap().join("NeuralAmpModelerCore"),
            env::current_dir()
                .unwrap()
                .join("NeuralAmpModelerCore")
                .join("NAM"),
            env::current_dir()
                .unwrap()
                .join("NeuralAmpModelerCore")
                .join("Dependencies")
                .join("eigen"),
            env::current_dir()
                .unwrap()
                .join("NeuralAmpModelerCore")
                .join("Dependencies")
                .join("nlohmann"),
        ]);

        for src in nam_sources {
            build.file(src);
        }

        build
            .std("c++20")
            .define("NOMINMAX", None)
            .define("WIN32_LEAN_AND_MEAN", None)
            .define("NAM_SAMPLE_FLOAT", None)
            .compile("nam_wrapper");

        println!("cargo:rerun-if-changed=src/nam.rs");
        println!("cargo:rerun-if-changed=src/cpp/nam_wrapper.cpp");
        println!("cargo:rerun-if-changed=src/cpp/nam_wrapper.h");
        println!("cargo:rerun-if-changed=NeuralAmpModelerCore/NAM");
    }

    // Generate presets inclusion file
    let presets_dir = PathBuf::from("src").join("resource").join("presets");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut generated_code = String::from("pub static PRESET_JSONS: &[&str] = &[\n");

    fn collect_json_files(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.path());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    collect_json_files(&path, files);
                } else if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("json")).unwrap_or(false) {
                    files.push(path);
                }
            }
        }
    }

    let mut preset_paths = Vec::new();
    collect_json_files(&presets_dir, &mut preset_paths);
    for path in preset_paths {
        if let Ok(absolute_path) = std::fs::canonicalize(&path) {
            let path_str = absolute_path.to_str().unwrap().replace("\\", "/");
            generated_code.push_str(&format!("    include_str!(\"{}\"),\n", path_str));
        }
    }
    generated_code.push_str("];\n");
    std::fs::write(out_dir.join("presets_generated.rs"), generated_code).unwrap();
    println!("cargo:rerun-if-changed=src/resource/presets");
}
