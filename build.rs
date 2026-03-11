use std::env;
use std::path::PathBuf;

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
}
