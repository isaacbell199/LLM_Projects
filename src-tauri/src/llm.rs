//! LLM integration using llama.cpp for GGUF model inference
//!
//! Version 0.3.1 - CORRECTED FOR llama-cpp-2 v0.1.140
//! ================================
//! CORRECTIONS IMPORTANTES:
//! 1. str_to_token utilise AddBos::Always
//! 2. token_to_piece avec decoder UTF-8
//! 3. Box::leak pour résoudre le lifetime de LlamaModel
//! 4. Reset KV cache between generations
//! 5. Proper batch handling for token generation
//! 6. GLOBAL SHARED BACKEND - fixes BackendAlreadyInitialized error
//! ================================

use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use crate::models::{
    GenerationChunk, GenerationRequest, HardwareInfo, LLMSettings, ModelInfo,
};
use crate::enrichment::{self, EnrichmentConfig, GenerationMode};
use crate::memory::{
    BatchConfig, CompressionStrategy, MemoryInfo, SlidingContextWindow,
    SlidingWindowConfig, ChunkPriority, compress_context,
};

// ============================================================================
// LLAMA.CPP IMPORTS (Version 0.1.140 EXACT)
#[cfg(feature = "llama-native")]
use llama_cpp_2::llama_backend::LlamaBackend;

#[cfg(feature = "llama-native")]
use llama_cpp_2::llama_batch::LlamaBatch;

#[cfg(feature = "llama-native")]
use llama_cpp_2::model::LlamaModel;

#[cfg(feature = "llama-native")]
use llama_cpp_2::model::params::LlamaModelParams;

#[cfg(feature = "llama-native")]
use llama_cpp_2::context::LlamaContext;

#[cfg(feature = "llama-native")]
use llama_cpp_2::context::params::LlamaContextParams;

#[cfg(feature = "llama-native")]
use llama_cpp_2::sampling::LlamaSampler;

#[cfg(feature = "llama-native")]
use llama_cpp_2::token::LlamaToken;
// ============================================================================

/// Check if AVX is available
pub fn is_avx_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        is_x86_feature_detected!("avx")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Check if AVX2 is available
pub fn is_avx2_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Check if AVX-512 is available
pub fn is_avx512_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        is_x86_feature_detected!("avx512f")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Check if FMA is available
pub fn is_fma_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Get CPU optimization level
pub fn get_optimization_info() -> String {
    if is_avx512_available() {
        "AVX-512 (Best)".to_string()
    } else if is_avx2_available() {
        "AVX2 (Excellent)".to_string()
    } else if is_avx_available() {
        "AVX (Good)".to_string()
    } else {
        "SSE (Basic)".to_string()
    }
}

/// Get recommended thread count
fn get_recommended_threads() -> usize {
    let physical_cores = num_cpus::get_physical();
    physical_cores.min(8).max(2)
}

// ============================================================================
// GPU Backend Detection
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    Cpu,
    Cuda,
    Vulkan,
    Metal,
}

impl std::fmt::Display for GpuBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuBackend::Cpu => write!(f, "CPU"),
            GpuBackend::Cuda => write!(f, "CUDA (NVIDIA)"),
            GpuBackend::Vulkan => write!(f, "Vulkan (AMD/Intel)"),
            GpuBackend::Metal => write!(f, "Metal (Apple)"),
        }
    }
}

pub fn detect_best_backend() -> GpuBackend {
    #[cfg(target_os = "macos")]
    {
        let sys = sysinfo::System::new();
        if sys.cpu_arch().contains("arm") {
            return GpuBackend::Metal;
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        if let Ok(output) = std::process::Command::new("nvidia-smi").output() {
            if output.status.success() {
                return GpuBackend::Cuda;
            }
        }
    }

    GpuBackend::Cpu
}

pub fn get_recommended_gpu_layers(vram_gb: Option<f64>, model_size_gb: f64) -> i32 {
    match vram_gb {
        Some(vram) => {
            let usable_vram = vram * 0.8;
            (usable_vram / model_size_gb * 32.0).min(100.0) as i32
        }
        None => 0,
    }
}

// ============================================================================
// Native llama.cpp Model Wrapper (Version 0.1.140)
// ============================================================================

/// Global shared backend - can only be initialized ONCE by llama.cpp
/// Using OnceLock ensures thread-safe one-time initialization
#[cfg(feature = "llama-native")]
static GLOBAL_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();

#[cfg(feature = "llama-native")]
pub struct NativeModel {
    /// Model reference with 'static lifetime (Box::leak)
    model: RwLock<Option<&'static LlamaModel>>,
    /// Context with 'static lifetime tied to model
    context: RwLock<Option<LlamaContext<'static>>>,
    /// Path to the loaded model file
    path: RwLock<Option<String>>,
    /// Track the number of tokens in KV cache for proper sequence management
    n_tokens: RwLock<i32>,
}

#[cfg(feature = "llama-native")]
unsafe impl Send for NativeModel {}
#[cfg(feature = "llama-native")]
unsafe impl Sync for NativeModel {}

#[cfg(feature = "llama-native")]
impl NativeModel {
    pub fn new() -> Self {
        // Backend is now global, no per-instance storage needed
        Self {
            model: RwLock::new(None),
            context: RwLock::new(None),
            path: RwLock::new(None),
            n_tokens: RwLock::new(0),
        }
    }

    /// Get or initialize the global shared backend
     fn get_backend() -> Result<Arc<LlamaBackend>> {
        if let Some(backend) = GLOBAL_BACKEND.get() {
            return Ok(Arc::clone(backend));
        }
        log::info!("Initializing GLOBAL llama backend (only once)...");
        let backend = LlamaBackend::init()
            .map_err(|e| anyhow!("Failed to initialize llama backend: {:?}", e))?;
        let arc_backend = Arc::new(backend);
        let _ = GLOBAL_BACKEND.set(Arc::clone(&arc_backend));
        Ok(arc_backend)
    }

    pub fn load(&self, path: &str, n_ctx: i32, n_gpu_layers: i32, n_threads: usize) -> Result<()> {
        // Get the global shared backend (initializes once)
        let backend = Self::get_backend()?;
        log::info!("Using global shared backend");

        log::info!("=== Loading Model ===");
        log::info!("Path: {}", path);
        log::info!("Context length: {}", n_ctx);
        log::info!("GPU layers: {}", n_gpu_layers);
        log::info!("Threads: {}", n_threads);

        // Verify file exists and get metadata
        let model_path = PathBuf::from(path);
        if !model_path.exists() {
            return Err(anyhow!("Model file not found: {}", path));
        }

        // Log file size
        if let Ok(metadata) = std::fs::metadata(&model_path) {
            let size_mb = metadata.len() / (1024 * 1024);
            log::info!("Model file size: {} MB", size_mb);
        }

        // Check file extension
        let extension = model_path.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if extension != "gguf" {
            log::warn!("File extension is '{}' but expected 'gguf'", extension);
        }

        // Use the canonical path to avoid any path issues
        let canonical_path = model_path.canonicalize()
            .map_err(|e| anyhow!("Failed to resolve path: {:?}", e))?;
        log::info!("Canonical path: {}", canonical_path.display());

        // Build model parameters
        let model_params = LlamaModelParams::default()
            .with_n_gpu_layers(n_gpu_layers as u32);

        log::info!("Attempting to load model with llama.cpp...");

        // Load model
        let model = match LlamaModel::load_from_file(&backend, &canonical_path, &model_params) {
            Ok(m) => {
                log::info!("Model loaded successfully into memory");
                m
            }
            Err(e) => {
                log::error!("Model loading failed: {:?}", e);
                log::error!("This could be due to:");
                log::error!("  1. Corrupted or incompatible GGUF file");
                log::error!("  2. Insufficient memory");
                log::error!("  3. Unsupported model architecture");
                log::error!("  4. Missing AVX/AVX2 support on CPU");
                return Err(anyhow!("Failed to load model: {:?}. The GGUF file may be corrupted or use an unsupported format.", e));
            }
        };

        // Build context parameters
        let context_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(n_ctx as u32))
            .with_n_threads(n_threads as i32);

        log::info!("Creating context with {} threads...", n_threads);

        // Box::leak for 'static lifetime
        let model_box = Box::new(model);
        let model_ref: &'static LlamaModel = Box::leak(model_box);

        // Create context
        let context = match model_ref.new_context(&backend, context_params) {
            Ok(c) => {
                log::info!("Context created successfully");
                c
            }
            Err(e) => {
                log::error!("Context creation failed: {:?}", e);
                // Try to clean up the leaked model
                // Note: We can't easily recover from Box::leak, so we just log
                return Err(anyhow!("Failed to create context: {:?}. Try reducing context length.", e));
            }
        };

        // Store model and context
        *self.model.write() = Some(model_ref);
        *self.context.write() = Some(context);
        *self.path.write() = Some(path.to_string());
        *self.n_tokens.write() = 0;

        log::info!("=== Model Ready ===");
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.model.read().is_some() && self.context.read().is_some()
    }

    pub fn unload(&self) {
        log::info!("Unloading model...");

        // Drop context first
        *self.context.write() = None;

        // Reconstruct Box to drop model properly
        if let Some(model_ref) = self.model.write().take() {
            unsafe {
                let _ = Box::from_raw(model_ref as *const LlamaModel as *mut LlamaModel);
            }
        }

        *self.path.write() = None;
        *self.n_tokens.write() = 0;

        log::info!("Model unloaded");
    }

    /// Reset the KV cache for a new generation
    pub fn reset_context(&self) -> Result<()> {
        let mut context_guard = self.context.write();
        let context = context_guard.as_mut()
            .ok_or_else(|| anyhow!("No context available"))?;

        // Clear the KV cache (returns unit type, no error handling needed)
        context.clear_kv_cache();

        *self.n_tokens.write() = 0;

        log::debug!("KV cache reset");
        Ok(())
    }

    pub fn generate(
        &self,
        prompt: &str,
        settings: &LLMSettings,
        app_handle: &AppHandle,
        should_stop: &RwLock<bool>,
    ) -> Result<String> {
        // SAFETY CHECK: Verify model is loaded
        if !self.is_loaded() {
            return Err(anyhow!("No model loaded - cannot generate"));
        }

        log::info!("=== Starting Generation ===");
        log::info!("Prompt length: {} chars", prompt.len());

        // Reset KV cache for new generation
        self.reset_context()?;

        let model_ref = self.model.read()
            .ok_or_else(|| anyhow!("No model loaded"))?;

        let mut context_guard = self.context.write();
        let context = context_guard.as_mut()
            .ok_or_else(|| anyhow!("No context available"))?;

        // SAFETY CHECK: Empty prompt
        if prompt.trim().is_empty() {
            return Err(anyhow!("Empty prompt - nothing to generate"));
        }

        // Tokenize prompt
        let tokens = model_ref.str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| anyhow!("Tokenization failed: {:?}", e))?;

        log::info!("Prompt tokens: {}", tokens.len());

        // SAFETY CHECK: Token limit
        if tokens.len() >= settings.context_length as usize {
            return Err(anyhow!("Prompt too long: {} tokens (max: {})",
                tokens.len(), settings.context_length));
        }

        // Create batch for prompt
        let n_prompt_tokens = tokens.len();
        let max_new_tokens = settings.max_tokens as usize;
        let batch_size = n_prompt_tokens + max_new_tokens;

        let mut batch = LlamaBatch::new(batch_size, 0);

        // Add all prompt tokens to batch
        // IMPORTANT: Only the LAST token should have logits=true for generation
        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == n_prompt_tokens - 1;
            batch.add(*token, i as i32, &[0.into()], is_last)
                .map_err(|e| anyhow!("Batch add failed at token {}: {:?}", i, e))?;
        }

        log::debug!("Added {} tokens to batch (last token has logits=true)", n_prompt_tokens);

        // Decode the prompt
        context.decode(&mut batch)
            .map_err(|e| anyhow!("Initial decode failed: {:?}", e))?;

        log::info!("Prompt decoded successfully");

        // Track position for next tokens
        let mut n_past = n_prompt_tokens as i32;

        // Create sampler chain with proper parameters to prevent repetition loops
        // Uses temperature, top_k, top_p, min_p, and repetition penalties from settings
        let samplers = vec![
            LlamaSampler::temp(settings.temperature),
            LlamaSampler::top_k(settings.top_k as i32),
            LlamaSampler::top_p(settings.top_p, 1),
            LlamaSampler::penalties(
                64, // last_n_tokens
                settings.repeat_penalty,
                settings.frequency_penalty,
                settings.presence_penalty,
            ),
        ];
        let mut sampler = LlamaSampler::chain(samplers, true);
        
        log::info!("Sampler: temp={}, top_k={}, top_p={}, min_p={}, repeat_penalty={}",
            settings.temperature, settings.top_k, settings.top_p, settings.min_p, settings.repeat_penalty);

        let mut generated_text = String::new();
        let mut tokens_generated = 0;

        log::info!("Starting token generation (max: {})...", max_new_tokens);

        while tokens_generated < max_new_tokens {
            if *should_stop.read() {
                log::info!("Generation stopped by user");
                break;
            }

            // Sample next token from the last position
            let token = sampler.sample(context, batch.n_tokens() - 1);

            // Check for end of generation
            if model_ref.is_eog_token(token) {
                log::info!("End of generation token reached after {} tokens", tokens_generated);
                break;
            }

            // Convert token to text
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let piece = model_ref.token_to_piece(token, &mut decoder, true, None)
                .map_err(|e| anyhow!("Token conversion error at token {}: {:?}", tokens_generated, e))?;

            generated_text.push_str(&piece);
            tokens_generated += 1;

            // Emit chunk event
            let chunk = GenerationChunk {
                content: piece.clone(),
                done: false,
            };
            let _ = app_handle.emit("generation-chunk", &chunk);

            // Prepare batch for next token
            batch.clear();
            batch.add(token, n_past, &[0.into()], true)
                .map_err(|e| anyhow!("Batch add failed for generated token {}: {:?}", tokens_generated, e))?;

            // Decode next token
            context.decode(&mut batch)
                .map_err(|e| anyhow!("Decode failed at token {}: {:?}", tokens_generated, e))?;

            n_past += 1;
        }

        // Emit done event
        let chunk = GenerationChunk {
            content: String::new(),
            done: true,
        };
        let _ = app_handle.emit("generation-chunk", &chunk);

        log::info!("=== Generation Complete ===");
        log::info!("Generated {} tokens", tokens_generated);

        Ok(generated_text)
    }

    /// Speculative Decoding: Use a draft model to accelerate generation
    ///
    /// # Arguments
    /// * `draft_model` - A smaller, faster model for speculative token generation
    /// * `n_draft` - Number of speculative tokens to generate per iteration (typically 4-8)
    ///
    /// # Algorithm
    /// 1. Draft model generates N speculative tokens
    /// 2. Main model verifies all tokens in a single forward pass
    /// 3. Accept tokens where draft distribution matches main distribution
    /// 4. Reject and resample from main model where they differ
    /// 5. Repeat until generation complete
    pub fn generate_speculative(
        &self,
        draft_model: &NativeModel,
        prompt: &str,
        settings: &LLMSettings,
        app_handle: &AppHandle,
        should_stop: &RwLock<bool>,
        n_draft: usize,
    ) -> Result<String> {
        // SAFETY CHECK: Verify both models are loaded
        if !self.is_loaded() {
            return Err(anyhow!("Main model not loaded - cannot generate"));
        }
        if !draft_model.is_loaded() {
            return Err(anyhow!("Draft model not loaded - cannot generate with speculative decoding"));
        }

        log::info!("=== Starting Speculative Decoding ===");
        log::info!("Prompt length: {} chars", prompt.len());
        log::info!("Draft tokens per iteration: {}", n_draft);

        // SAFETY CHECK: Empty prompt
        if prompt.trim().is_empty() {
            return Err(anyhow!("Empty prompt - nothing to generate"));
        }

        // Reset KV caches for both models
        self.reset_context()?;
        draft_model.reset_context()?;

        // Get model references
        let main_model_ref = self.model.read()
            .ok_or_else(|| anyhow!("No main model loaded"))?;
        let draft_model_ref = draft_model.model.read()
            .ok_or_else(|| anyhow!("No draft model loaded"))?;

        // Tokenize prompt with main model (draft should have same tokenizer)
        let tokens = main_model_ref.str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| anyhow!("Tokenization failed: {:?}", e))?;

        log::info!("Prompt tokens: {}", tokens.len());

        // SAFETY CHECK: Token limit
        if tokens.len() >= settings.context_length as usize {
            return Err(anyhow!("Prompt too long: {} tokens (max: {})",
                tokens.len(), settings.context_length));
        }

        let n_prompt_tokens = tokens.len();
        let max_new_tokens = settings.max_tokens as usize;

        // Initialize both contexts with the prompt
        {
            let mut main_context = self.context.write();
            let main_ctx = main_context.as_mut()
                .ok_or_else(|| anyhow!("No main context available"))?;

            let mut draft_context = draft_model.context.write();
            let draft_ctx = draft_context.as_mut()
                .ok_or_else(|| anyhow!("No draft context available"))?;

            // Create batches for prompt
            let mut main_batch = LlamaBatch::new(n_prompt_tokens + n_draft + 1, 0);
            let mut draft_batch = LlamaBatch::new(n_prompt_tokens + n_draft + 1, 0);

            // Add all prompt tokens to both batches
            for (i, token) in tokens.iter().enumerate() {
                let is_last = i == n_prompt_tokens - 1;
                main_batch.add(*token, i as i32, &[0.into()], is_last)
                    .map_err(|e| anyhow!("Main batch add failed at token {}: {:?}", i, e))?;
                draft_batch.add(*token, i as i32, &[0.into()], is_last)
                    .map_err(|e| anyhow!("Draft batch add failed at token {}: {:?}", i, e))?;
            }

            // Decode prompt with both models
            main_ctx.decode(&mut main_batch)
                .map_err(|e| anyhow!("Main model prompt decode failed: {:?}", e))?;
            draft_ctx.decode(&mut draft_batch)
                .map_err(|e| anyhow!("Draft model prompt decode failed: {:?}", e))?;
        }

        // Track position
        let mut n_past = n_prompt_tokens as i32;
        let mut generated_text = String::new();
        let mut tokens_generated = 0;
        let mut accepted_count = 0usize;
        let mut total_draft_count = 0usize;

        log::info!("Starting speculative decoding (max: {} tokens)...", max_new_tokens);

        // Main generation loop
        while tokens_generated < max_new_tokens {
            if *should_stop.read() {
                log::info!("Generation stopped by user");
                break;
            }

            // === Phase 1: Draft model generates speculative tokens ===
            let draft_tokens: Vec<LlamaToken> = {
                let mut draft_context = draft_model.context.write();
                let draft_ctx = draft_context.as_mut()
                    .ok_or_else(|| anyhow!("No draft context available"))?;

                // Use the same sampler settings for draft model (higher temp for diversity)
                let draft_samplers = vec![
    LlamaSampler::temp(settings.temperature * 1.1), // Slightly higher temp for draft
    LlamaSampler::top_k(settings.top_k as i32),
    LlamaSampler::top_p(settings.top_p, 1),
];
let mut draft_sampler = LlamaSampler::chain(draft_samplers, true);
                 
                let mut speculative_tokens = Vec::with_capacity(n_draft);
                let mut draft_batch = LlamaBatch::new(n_draft + 1, 0);

                let mut current_pos = n_past;

                for _ in 0..n_draft {
                    // Sample from draft model
                    let token = draft_sampler.sample(draft_ctx, -1);

                    // Check for end of generation
                    if draft_model_ref.is_eog_token(token) {
                        break;
                    }

                    speculative_tokens.push(token);

                    // Add token to batch for next decode
                    draft_batch.clear();
                    draft_batch.add(token, current_pos, &[0.into()], true)
                        .map_err(|e| anyhow!("Draft batch add failed: {:?}", e))?;

                    // Decode to update KV cache
                    draft_ctx.decode(&mut draft_batch)
                        .map_err(|e| anyhow!("Draft decode failed: {:?}", e))?;

                    current_pos += 1;
                }

                speculative_tokens
            };

            let n_speculative = draft_tokens.len();
            total_draft_count += n_speculative;

            if n_speculative == 0 {
                // Draft model returned EOG, we're done
                break;
            }

            // === Phase 2: Main model verifies speculative tokens ===
            {
                let mut main_context = self.context.write();
                let main_ctx = main_context.as_mut()
                    .ok_or_else(|| anyhow!("No main context available"))?;

                // Use the preset sampler settings for main model verification
                let main_samplers = vec![
    LlamaSampler::temp(settings.temperature),
    LlamaSampler::top_k(settings.top_k as i32),
    LlamaSampler::top_p(settings.top_p, 1),
    LlamaSampler::penalties(
        64, 
        settings.repeat_penalty,
        settings.frequency_penalty,
        settings.presence_penalty,
    ),
];
let mut main_sampler = LlamaSampler::chain(main_samplers, true);
               
                // Create batch with all speculative tokens
                let mut verify_batch = LlamaBatch::new(n_speculative + 1, 0);
                for (i, &token) in draft_tokens.iter().enumerate() {
                    verify_batch.add(token, n_past + i as i32, &[0.into()], true)
                        .map_err(|e| anyhow!("Verify batch add failed at {}: {:?}", i, e))?;
                }

                // Decode all speculative tokens at once
                main_ctx.decode(&mut verify_batch)
                    .map_err(|e| anyhow!("Main verify decode failed: {:?}", e))?;

                // === Phase 3: Accept/Reject tokens ===
                let mut n_accepted = 0i32;

                for (i, &draft_token) in draft_tokens.iter().enumerate() {
                    // Get main model's preferred token at this position
                    let main_token = main_sampler.sample(main_ctx, i as i32);

                    // Check for end of generation
                    if main_model_ref.is_eog_token(main_token) {
                        log::info!("End of generation token reached after {} tokens", tokens_generated + n_accepted as usize);

                        // Emit done event and return
                        let chunk = GenerationChunk {
                            content: String::new(),
                            done: true,
                        };
                        let _ = app_handle.emit("generation-chunk", &chunk);

                        log::info!("=== Speculative Decoding Complete ===");
                        log::info!("Generated {} tokens total", tokens_generated + n_accepted as usize);
                        log::info!("Acceptance rate: {:.1}%",
                            if total_draft_count > 0 {
                                (accepted_count + n_accepted as usize) as f64 / total_draft_count as f64 * 100.0
                            } else { 0.0 });

                        return Ok(generated_text);
                    }

                    // Accept if main model agrees (greedy acceptance)
                    if main_token == draft_token {
                        // Accept the draft token
                        let mut decoder = encoding_rs::UTF_8.new_decoder();
                        let piece = main_model_ref.token_to_piece(main_token, &mut decoder, true, None)
                            .map_err(|e| anyhow!("Token conversion error: {:?}", e))?;

                        generated_text.push_str(&piece);
                        tokens_generated += 1;
                        n_accepted += 1;
                        accepted_count += 1;

                        // Emit chunk event
                        let chunk = GenerationChunk {
                            content: piece.clone(),
                            done: false,
                        };
                        let _ = app_handle.emit("generation-chunk", &chunk);

                        if tokens_generated >= max_new_tokens {
                            break;
                        }
                    } else {
                        // Reject draft token, use main model's token instead
                        log::debug!("Token rejected at position {}, using main model's choice", i);

                        let mut decoder = encoding_rs::UTF_8.new_decoder();
                        let piece = main_model_ref.token_to_piece(main_token, &mut decoder, true, None)
                            .map_err(|e| anyhow!("Token conversion error: {:?}", e))?;

                        generated_text.push_str(&piece);
                        tokens_generated += 1;
                        n_accepted += 1; // We still accept one token (from main model)

                        // Emit chunk event
                        let chunk = GenerationChunk {
                            content: piece.clone(),
                            done: false,
                        };
                        let _ = app_handle.emit("generation-chunk", &chunk);

                        // Rollback draft model's KV cache to this position
                        {
                            let mut draft_context = draft_model.context.write();
                            if let Some(draft_ctx) = draft_context.as_mut() {
                                // Rollback draft context by clearing and re-decoding
                                // Note: llama.cpp doesn't have direct KV cache rollback,
                                // so we need to reset and catch up
                                draft_ctx.clear_kv_cache();
                            }
                        }

                        // Sync draft model: decode all accepted tokens
                        {
                            let mut draft_context = draft_model.context.write();
                            if let Some(draft_ctx) = draft_context.as_mut() {
                                // Re-decode prompt + accepted tokens
                                let mut sync_batch = LlamaBatch::new(n_prompt_tokens + tokens_generated, 0);

                                // Re-add prompt tokens
                                for (i, token) in tokens.iter().enumerate() {
                                    sync_batch.add(*token, i as i32, &[0.into()], false)
                                        .map_err(|e| anyhow!("Sync batch failed: {:?}", e))?;
                                }

                                // Re-decode prompt
                                draft_ctx.decode(&mut sync_batch)
                                    .map_err(|e| anyhow!("Draft sync decode failed: {:?}", e))?;
                            }
                        }

                        break; // Stop accepting after rejection
                    }
                }

                n_past += n_accepted;
            }
        }

        // Emit done event
        let chunk = GenerationChunk {
            content: String::new(),
            done: true,
        };
        let _ = app_handle.emit("generation-chunk", &chunk);

        log::info!("=== Speculative Decoding Complete ===");
        log::info!("Generated {} tokens total", tokens_generated);
        log::info!("Acceptance rate: {:.1}%",
            if total_draft_count > 0 {
                accepted_count as f64 / total_draft_count as f64 * 100.0
            } else { 0.0 });

        Ok(generated_text)
    }
}

// ============================================================================
// Fallback when llama-native is disabled
// ============================================================================

#[cfg(not(feature = "llama-native"))]
pub struct NativeModel;

#[cfg(not(feature = "llama-native"))]
impl NativeModel {
    pub fn new() -> Self { Self }
    pub fn load(&self, _path: &str, _n_ctx: i32, _n_gpu_layers: i32, _n_threads: usize) -> Result<()> {
        Err(anyhow!("llama-native feature not enabled - please rebuild with --features llama-native"))
    }
    pub fn is_loaded(&self) -> bool { false }
    pub fn unload(&self) {}
    pub fn reset_context(&self) -> Result<()> { Ok(()) }
    pub fn generate(&self, _prompt: &str, _settings: &LLMSettings, _app_handle: &AppHandle, _should_stop: &RwLock<bool>) -> Result<String> {
        Err(anyhow!("llama-native feature not enabled"))
    }
    pub fn generate_speculative(
        &self,
        _draft_model: &NativeModel,
        _prompt: &str,
        _settings: &LLMSettings,
        _app_handle: &AppHandle,
        _should_stop: &RwLock<bool>,
        _n_draft: usize,
    ) -> Result<String> {
        Err(anyhow!("llama-native feature not enabled"))
    }
}

// ============================================================================
// Global LLM State
// ============================================================================

pub struct LlmState {
    pub settings: RwLock<LLMSettings>,
    pub is_generating: RwLock<bool>,
    pub should_stop: RwLock<bool>,
    pub context_window: RwLock<SlidingContextWindow>,
    pub batch_config: RwLock<BatchConfig>,
    pub memory_info: RwLock<Option<MemoryInfo>>,
    pub gpu_backend: GpuBackend,
    pub n_threads: usize,
    pub model_loaded: RwLock<bool>,
    pub draft_model_path: RwLock<Option<String>>,
    pub draft_model_loaded: RwLock<bool>,
    pub duo_model_enabled: RwLock<bool>,
    pub native_model: NativeModel,
    pub native_draft_model: NativeModel,
}

impl LlmState {
    pub fn new() -> Self {
        let mem_info = MemoryInfo::current();
        let batch_config = mem_info.recommended_batch_config();
        let context_config = SlidingWindowConfig {
            max_tokens: mem_info.recommended_context_tokens(),
            ..Default::default()
        };

        let gpu_backend = detect_best_backend();
        let n_threads = get_recommended_threads();

        log::info!("=== NexaStory LLM State ===");
        log::info!("CPU Optimization: {}", get_optimization_info());
        log::info!("Threads: {}", n_threads);
        log::info!("GPU Backend: {}", gpu_backend);

        Self {
            settings: RwLock::new(LLMSettings::default()),
            is_generating: RwLock::new(false),
            should_stop: RwLock::new(false),
            context_window: RwLock::new(SlidingContextWindow::new(context_config)),
            batch_config: RwLock::new(batch_config),
            memory_info: RwLock::new(Some(mem_info)),
            gpu_backend,
            n_threads,
            model_loaded: RwLock::new(false),
            draft_model_path: RwLock::new(None),
            draft_model_loaded: RwLock::new(false),
            duo_model_enabled: RwLock::new(false),
            native_model: NativeModel::new(),
            native_draft_model: NativeModel::new(),
        }
    }

    pub fn update_memory_state(&self) {
        let mem_info = MemoryInfo::current();
        *self.memory_info.write() = Some(mem_info);
    }

    pub fn add_context(&self, text: &str, priority: ChunkPriority, source: &str) {
        self.context_window.write().add_text(text, priority, source);
    }

    pub fn clear_context(&self) {
        self.context_window.write().clear();
    }

    pub fn get_optimized_context(&self, max_tokens: usize) -> String {
        let window = self.context_window.read();
        let context = window.get_context();
        if window.token_count() <= max_tokens {
            context
        } else {
            compress_context(&context, max_tokens, CompressionStrategy::KeepEssential)
        }
    }
}

impl Default for LlmState {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Hardware Information
// ============================================================================

pub fn get_hardware_info() -> HardwareInfo {
    let sys = sysinfo::System::new_all();
    let cpu_cores = sys.cpus().len();
    let total_memory = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let available_memory = sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

    let best_backend = detect_best_backend();
    let has_gpu = best_backend != GpuBackend::Cpu;
    let (gpu_name, gpu_memory) = if has_gpu { get_gpu_info() } else { (None, None) };

    HardwareInfo {
        cpu_cores,
        cpu_threads: cpu_cores,
        total_memory_gb: total_memory,
        available_memory_gb: available_memory,
        has_gpu,
        gpu_name,
        gpu_memory_gb: gpu_memory,
        recommended_threads: get_recommended_threads(),
        recommended_batch_size: if total_memory > 16.0 { 1024 } else { 512 },
        best_backend: Some(best_backend.to_string()),
    }
}

fn get_gpu_info() -> (Option<String>, Option<f64>) {
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=name,memory.total", "--format=csv,noheader"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = stdout.trim().split(',').collect();
            if parts.len() >= 2 {
                let name = parts[0].trim().to_string();
                let mem = parts[1].trim().split_whitespace().next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|mb| mb / 1024.0);
                return (Some(name), mem);
            }
        }
    }
    (None, None)
}

// ============================================================================
// Model Scanning
// ============================================================================

pub fn scan_models_directory(path: &str) -> Result<Vec<ModelInfo>> {
    let mut models = Vec::new();
    let path = PathBuf::from(path);

    if !path.exists() { return Ok(models); }

    for entry in WalkDir::new(&path).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        let file_path = entry.path();
        if let Some(ext) = file_path.extension() {
            if ext.eq_ignore_ascii_case("gguf") {
                if let Ok(metadata) = std::fs::metadata(file_path) {
                    let name = file_path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Unknown".to_string());

                    models.push(ModelInfo {
                        name,
                        path: file_path.to_string_lossy().to_string(),
                        size_mb: metadata.len() / (1024 * 1024),
                        parameters: None,
                        quantization: None,
                        context_length: 4096,
                    });
                }
            }
        }
    }
    Ok(models)
}

pub fn get_model_info(path: &str) -> Result<ModelInfo> {
    let file_path = PathBuf::from(path);
    if !file_path.exists() {
        return Err(anyhow!("Model file not found"));
    }

    let metadata = std::fs::metadata(&file_path)?;
    let name = file_path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    Ok(ModelInfo {
        name,
        path: path.to_string(),
        size_mb: metadata.len() / (1024 * 1024),
        parameters: None,
        quantization: None,
        context_length: 4096,
    })
}

// ============================================================================
// Model Loading and Generation
// ============================================================================

pub fn load_model(state: &LlmState, path: &str, settings: &LLMSettings) -> Result<()> {
    log::info!("=== Loading Model ===");
    log::info!("Path: {}", path);

    let model_path = PathBuf::from(path);
    if !model_path.exists() {
        return Err(anyhow!("Model file not found: {}", path));
    }

    // Update settings with model path
    state.settings.write().model_path = Some(path.to_string());

    // Load the native model
    state.native_model.load(path, settings.context_length, settings.gpu_layers, state.n_threads)?;

    *state.model_loaded.write() = true;

    log::info!("=== Model Loaded Successfully ===");
    Ok(())
}

pub fn unload_model(state: &LlmState) {
    log::info!("Unloading model...");

    state.native_model.unload();
    state.settings.write().model_path = None;
    state.clear_context();
    *state.model_loaded.write() = false;

    log::info!("Model unloaded");
}

pub fn generate_text(state: &LlmState, app_handle: &AppHandle, request: GenerationRequest) -> Result<()> {
    // Check if model is loaded
    if !*state.model_loaded.read() {
        let error_msg = "No model loaded - please load a model first";
        log::error!("{}", error_msg);

        // Emit error to frontend
        let _ = app_handle.emit("generation-chunk", &GenerationChunk {
            content: format!("[Error: {}]", error_msg),
            done: true,
        });

        return Err(anyhow!("{}", error_msg));
    }

    // Check if duo model (speculative decoding) mode is enabled
    let use_speculative = *state.duo_model_enabled.read() && *state.draft_model_loaded.read();

    if use_speculative {
        log::info!("=== Starting Speculative Decoding Generation ===");
    } else {
        log::info!("=== Starting Standard Generation ===");
    }

    *state.is_generating.write() = true;
    *state.should_stop.write() = false;

    // Get current settings and apply request overrides
    let mut settings = state.settings.read().clone();

    // Apply sampling parameter overrides from request (from preset)
    if let Some(temp) = request.temperature {
        settings.temperature = temp;
    }
    if let Some(tokens) = request.max_tokens {
        settings.max_tokens = tokens;
    }
    if let Some(top_p) = request.top_p {
        settings.top_p = top_p;
    }
    if let Some(top_k) = request.top_k {
        settings.top_k = top_k;
    }
    if let Some(min_p) = request.min_p {
        settings.min_p = min_p;
    }
    if let Some(rp) = request.repeat_penalty {
        settings.repeat_penalty = rp;
    }
    if let Some(fp) = request.frequency_penalty {
        settings.frequency_penalty = fp;
    }
    if let Some(pp) = request.presence_penalty {
        settings.presence_penalty = pp;
    }

    log::info!("Generation params: temp={}, top_p={}, top_k={}, min_p={}, repeat_penalty={}",
        settings.temperature, settings.top_p, settings.top_k, settings.min_p, settings.repeat_penalty);

    // Build enriched prompt
    let prompt = build_enriched_prompt_optimized(&request, &settings);

    log::debug!("Final prompt length: {} chars", prompt.len());

    // Generate using appropriate method
    let result = if use_speculative {
        // Use speculative decoding with draft model
        // n_draft = 4 is a good balance between speed and acceptance rate
        state.native_model.generate_speculative(
            &state.native_draft_model,
            &prompt,
            &settings,
            app_handle,
            &state.should_stop,
            4, // Number of speculative tokens per iteration
        )
    } else {
        // Standard single-model generation
        state.native_model.generate(&prompt, &settings, app_handle, &state.should_stop)
    };

    match result {
        Ok(text) => {
            log::info!("Generation completed: {} chars generated", text.len());
        }
        Err(e) => {
            log::error!("Generation failed: {}", e);
            let _ = app_handle.emit("generation-chunk", &GenerationChunk {
                content: format!("[Error: {}]", e),
                done: true,
            });
        }
    }

    *state.is_generating.write() = false;

    log::info!("=== Generation Session Complete ===");
    Ok(())
}

pub fn stop_generation(state: &LlmState) {
    log::info!("Stop generation requested");
    *state.should_stop.write() = true;
}

pub fn build_enriched_prompt_optimized(request: &GenerationRequest, _settings: &LLMSettings) -> String {
    let mode = GenerationMode::from_string(&request.mode);
    let config = EnrichmentConfig {
        mode,
        start_phrase: request.start_phrase.clone(),
        end_phrase: request.end_phrase.clone(),
        language: request.language.clone().unwrap_or_else(|| "en".to_string()),
        custom_style: request.custom_style_instruction.clone(),
    };

    let mut prompt = String::new();

    // Add model-specific system prompt if provided
    if let Some(system_prompt) = &request.system_prompt {
        if !system_prompt.trim().is_empty() {
            prompt.push_str(system_prompt);
            prompt.push_str("\n\n---\n\n");
        }
    }

    // === BUILD COMPREHENSIVE CONTEXT ===
    prompt.push_str("[STORY CONTEXT]\n\n");

    // Add project settings if available
    if let Some(settings) = &request.project_settings {
        if let Some(ref genres) = settings.genres {
            if !genres.is_empty() {
                prompt.push_str(&format!("Genre: {}\n", genres));
            }
        }
        if let Some(ref themes) = settings.themes {
            if !themes.is_empty() {
                prompt.push_str(&format!("Themes: {}\n", themes));
            }
        }
        if let Some(ref style) = settings.writing_style {
            if !style.is_empty() {
                prompt.push_str(&format!("Writing Style: {}\n", style));
            }
        }
        if let Some(ref tone) = settings.tone_preferences {
            if !tone.is_empty() {
                prompt.push_str(&format!("Tone: {}\n", tone));
            }
        }
        if let Some(ref pov) = settings.narrative_pov {
            if !pov.is_empty() {
                prompt.push_str(&format!("Narrative POV: {}\n", pov));
            }
        }
        prompt.push('\n');
    }

    // Add custom genre instruction
    if let Some(ref genre_name) = request.custom_genre_name {
        prompt.push_str(&format!("[GENRE: {}]\n", genre_name));
        if let Some(ref instruction) = request.custom_genre_instruction {
            prompt.push_str(&format!("{}\n", instruction));
        }
        prompt.push('\n');
    }

    // Add custom style instruction
    if let Some(ref style_name) = request.custom_style_name {
        prompt.push_str(&format!("[WRITING STYLE: {}]\n", style_name));
        if let Some(ref instruction) = request.custom_style_instruction {
            prompt.push_str(&format!("{}\n", instruction));
        }
        prompt.push('\n');
    }

    // Add selected tone
    if let Some(ref tone) = request.selected_tone {
        prompt.push_str(&format!("[TONE: {}]\n", tone));
        if let Some(ref instruction) = request.custom_tone_instruction {
            prompt.push_str(&format!("{}\n", instruction));
        }
        prompt.push('\n');
    }

    // Add positive prompt if provided
    if let Some(ref positive) = request.positive_prompt {
        if !positive.is_empty() {
            prompt.push_str(&format!("[CREATIVE DIRECTION]\n{}\n\n", positive));
        }
    }

    // Add negative prompt if provided (things to avoid)
    if let Some(ref negative) = request.negative_prompt {
        if !negative.is_empty() {
            prompt.push_str(&format!("[AVOID]\n{}\n\n", negative));
        }
    }

    // Add characters information
    if !request.characters.is_empty() {
        prompt.push_str("[CHARACTERS]\n");
        for character in &request.characters {
            prompt.push_str(&format!("- {} ", character.name));
            if let Some(ref role) = character.role {
                if !role.is_empty() {
                    prompt.push_str(&format!("({}): ", role));
                }
            } else {
                prompt.push_str(": ");
            }
            if let Some(ref desc) = character.background {
                if !desc.is_empty() {
                    prompt.push_str(desc);
                }
            }
            if let Some(ref personality) = character.personality {
                if !personality.is_empty() {
                    prompt.push_str(&format!(" Personality: {}", personality));
                }
            }
            if let Some(ref speech) = character.speech_pattern {
                if !speech.is_empty() {
                    prompt.push_str(&format!(" Speech: {}", speech));
                }
            }
            prompt.push('\n');
        }
        prompt.push('\n');
    }

    // Add locations information
    if !request.locations.is_empty() {
        prompt.push_str("[LOCATIONS]\n");
        for location in &request.locations {
            prompt.push_str(&format!("- {}", location.name));
            if let Some(ref loc_type) = location.location_type {
                if !loc_type.is_empty() {
                    prompt.push_str(&format!(" ({})", loc_type));
                }
            }
            if let Some(ref desc) = location.description {
                if !desc.is_empty() {
                    prompt.push_str(&format!(": {}", desc));
                }
            }
            if let Some(ref atmosphere) = location.atmosphere {
                if !atmosphere.is_empty() {
                    prompt.push_str(&format!(" Atmosphere: {}", atmosphere));
                }
            }
            prompt.push('\n');
        }
        prompt.push('\n');
    }

    // Add selected character focus
    if let Some(ref char_id) = request.selected_character_id {
        if let Some(character) = request.characters.iter().find(|c| &c.id == char_id) {
            prompt.push_str(&format!("[FOCUS CHARACTER: {}]\n", character.name));
            if let Some(ref desc) = character.background {
                prompt.push_str(&format!("{}\n", desc));
            }
            prompt.push('\n');
        }
    }

    // Add story context (previous text)
    if let Some(ctx) = &request.context {
        if !ctx.is_empty() {
            prompt.push_str("[STORY SO FAR]\n");
            prompt.push_str(ctx);
            prompt.push_str("\n\n");
        }
    }

    // Add the main prompt/instruction
    prompt.push_str("[GENERATION TASK]\n");
    prompt.push_str(&request.text);
    prompt.push_str("\n\nContinue: ");

    enrichment::wrap_enriched_prompt(&prompt, &config)
}

// ============================================================================
// Memory Management
// ============================================================================

pub fn get_memory_info() -> MemoryInfo {
    MemoryInfo::current()
}

pub fn get_recommended_memory_settings() -> (i32, i32, i32, String) {
    let mem = MemoryInfo::current();
    (mem.recommended_context_tokens() as i32, 512, 10, "balanced".to_string())
}

// ============================================================================
// Draft Model (Speculative Decoding)
// ============================================================================

/// Load a draft model for speculative decoding
pub fn load_draft_model(state: &LlmState, path: &str) -> Result<()> {
    log::info!("Loading draft model: {}", path);

    let model_path = PathBuf::from(path);
    if !model_path.exists() {
        return Err(anyhow!("Draft model file not found: {}", path));
    }

    let settings = state.settings.read();

    // Load draft model with same settings as main model
    state.native_draft_model.load(
        path,
        settings.context_length,
        settings.gpu_layers,
        state.n_threads,
    )?;

    *state.draft_model_path.write() = Some(path.to_string());
    *state.draft_model_loaded.write() = true;

    log::info!("Draft model loaded successfully!");
    Ok(())
}

/// Unload the draft model
pub fn unload_draft_model(state: &LlmState) {
    state.native_draft_model.unload();
    *state.draft_model_path.write() = None;
    *state.draft_model_loaded.write() = false;
    *state.duo_model_enabled.write() = false;
    log::info!("Draft model unloaded");
}

/// Enable or disable speculative decoding mode
pub fn set_duo_model_enabled(state: &LlmState, enabled: bool) {
    if enabled && !*state.draft_model_loaded.read() {
        log::warn!("Cannot enable duo model: draft model not loaded");
        return;
    }

    if enabled && !*state.model_loaded.read() {
        log::warn!("Cannot enable duo model: main model not loaded");
        return;
    }

    *state.duo_model_enabled.write() = enabled;
    log::info!("Duo model mode: {}", if enabled { "enabled" } else { "disabled" });
}

/// Get duo model status
pub fn get_duo_model_status(state: &LlmState) -> serde_json::Value {
    let main_loaded = *state.model_loaded.read();
    let draft_loaded = *state.draft_model_loaded.read();
    let enabled = *state.duo_model_enabled.read();
    let draft_path = state.draft_model_path.read().clone();
    let main_path = state.settings.read().model_path.clone();

    serde_json::json!({
        "enabled": enabled,
        "mainModelLoaded": main_loaded,
        "draftModelLoaded": draft_loaded,
        "ready": main_loaded && draft_loaded && enabled,
        "mainModelPath": main_path,
        "draftModelPath": draft_path
    })
}
