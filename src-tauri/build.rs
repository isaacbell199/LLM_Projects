fn main() {
    // Enable CPU-specific optimizations at compile time
    // This will auto-detect and enable AVX, AVX2, etc. based on the build machine
    
    #[cfg(target_arch = "x86_64")]
    {
        // Check for AVX2 support at compile time
        if is_x86_feature_detected!("avx2") {
            println!("cargo:rustc-env=LLAMA_AVX2=1");
            println!("cargo:warning=AVX2 detected and enabled for optimal performance");
        } else if is_x86_feature_detected!("avx") {
            println!("cargo:rustc-env=LLAMA_AVX=1");
            println!("cargo:warning=AVX detected and enabled");
        } else {
            println!("cargo:warning=No AVX/AVX2 detected - using SSE only");
        }
        
        // Check for FMA (Fused Multiply-Add)
        if is_x86_feature_detected!("fma") {
            println!("cargo:rustc-env=LLAMA_FMA=1");
        }
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        // Apple Silicon optimization
        println!("cargo:rustc-env=LLAMA_NEON=1");
        println!("cargo:warning=ARM NEON enabled for Apple Silicon");
    }
    
    // Print compilation info
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    
    tauri_build::build()
}
