// Build script for llama-cpp-sys
// This is a stub for now; actual llama.cpp compilation will be added when vendoring source

fn main() {
    #[cfg(feature = "llama-cpp")]
    {
        println!("cargo:warning=llama-cpp feature enabled but vendored source not yet available");
        println!("cargo:warning=See VENDOR.md for instructions on vendoring llama.cpp");

        // Future: Compile vendored llama.cpp C++ code
        // cc::Build::new()
        //     .cpp(true)
        //     .file("vendor/llama.cpp/llama.cpp")
        //     .file("vendor/llama.cpp/ggml.c")
        //     .compile("llama");

        // Tell cargo to link the library
        // println!("cargo:rustc-link-lib=static=llama");
    }

    // Rerun if build.rs changes
    println!("cargo:rerun-if-changed=build.rs");
}
