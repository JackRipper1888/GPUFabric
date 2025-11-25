use std::env;
use std::path::PathBuf;

fn main() {
    // Temporarily disable cbindgen to avoid syntax errors
    // let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    
    // Configure cbindgen directly, without relying on external config files
    // cbindgen::Builder::new()
    //     .with_crate(crate_dir)
    //     .with_language(cbindgen::Language::C)
    //     .with_pragma_once(true)
    //     .with_include_guard("GPUF_C_H")
    //     .with_documentation(true)
    //     .generate()
    //     .expect("Unable to generate bindings")
    //     .write_to_file("gpuf_c.h");
    
    // Get the target OS from Cargo environment variable
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    println!("cargo:warning=Target OS detected: {}", target_os);

    // Configure NVML library path for Windows target
    if target_os == "windows" {
        // Common NVIDIA NVML library locations on Windows
        let possible_paths = vec![
            r"C:\Program Files\NVIDIA Corporation\NVSMI",
            r"C:\Windows\System32",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.0\lib\x64",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.8\lib\x64",
            r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.7\lib\x64",
        ];
        
        // Check if NVML_LIB_PATH environment variable is set
        if let Ok(nvml_path) = env::var("NVML_LIB_PATH") {
            println!("cargo:rustc-link-search=native={}", nvml_path);
        } else {
            // Try to find nvml.lib in common locations
            // Note: checking path existence works only if cross-compiling on Windows or if paths are mapped
            // For cross-compilation from Linux, this usually won't find anything, which is fine
            for path in possible_paths {
                let nvml_lib = PathBuf::from(path).join("nvml.lib");
                if nvml_lib.exists() {
                    println!("cargo:rustc-link-search=native={}", path);
                    println!("cargo:warning=Found nvml.lib at: {}", path);
                    break;
                }
            }
        }
    }
    
    // Link OpenMP on Linux target explicitly (LLVM OpenMP)
    // This is required because llama.cpp is compiled with Clang and uses __kmpc_* symbols
    if target_os == "linux" {
        // 1. Check if LIBOMP_PATH environment variable is set
        if let Ok(libomp_path) = env::var("LIBOMP_PATH") {
            println!("cargo:rustc-link-search=native={}", libomp_path);
        } else {
            // 2. Check common LLVM library paths for libomp.so to avoid hardcoding specific versions
            let possible_llvm_paths = vec![
                "/usr/lib/llvm-19/lib",
                "/usr/lib/llvm-18/lib",
                "/usr/lib/llvm-17/lib",
                "/usr/lib/llvm-16/lib",
                "/usr/lib/llvm-15/lib",
                "/usr/lib/llvm-14/lib",
            ];

            for path in possible_llvm_paths {
                if std::path::Path::new(path).join("libomp.so").exists() {
                    println!("cargo:rustc-link-search=native={}", path);
                    break;
                }
            }
        }

        println!("cargo:rustc-link-lib=omp");
    }
    
    println!("cargo:rerun-if-changed=src/lib.rs");
}
