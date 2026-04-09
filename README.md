# NexaStory Desktop

AI-powered creative writing assistant that runs entirely locally with GGUF models.

## Features

- **Local AI Models** - Use any GGUF model, your data never leaves your computer
- **Duo Model (Speculative Decoding)** - Use two models for 2-4x faster generation
- **Native llama.cpp** - No external server required
- **GPU Acceleration** - CUDA, Metal, and Vulkan support
- **CPU Optimized** - AVX, AVX2, AVX-512 auto-detection
- **Memory Efficient** - Sliding context window and adaptive batching

## System Requirements

| Platform | Requirements |
|----------|--------------|
| Windows | Windows 10/11 x64 |
| macOS | macOS 10.15+ (Intel or Apple Silicon) |
| Linux | Ubuntu 20.04+ or equivalent |

## Recommended Models

### Single Model
- Llama 3.2 3B Q4_K_M
- Qwen 2.5 7B Q4_K_M
- Mistral 7B Q4_K_M

### Duo Model Pairs (Speculative Decoding)
| Main Model | Draft Model | Speed Boost |
|------------|-------------|-------------|
| Llama 3.2 3B | Llama 3.2 1B | 2-3x |
| Qwen 2.5 7B | Qwen 2.5 1.5B | 2-4x |
| Mistral 7B | Mistral 0.5B | 2-3x |

## Installation

### Download Release
Download the latest release from the [Releases](../../releases) page.

### Build from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/nexastory-desktop.git
cd nexastory-desktop

# Install dependencies
bun install

# Build for your platform
bun run tauri build --release --features llama-native

# Or with GPU support
bun run tauri build --release --features llama-cuda    # NVIDIA
bun run tauri build --release --features llama-metal   # macOS
bun run tauri build --release --features llama-vulkan  # AMD/Intel
```

## Development

```bash
# Start development server
bun run tauri dev

# Run tests
cargo test --all-features

# Lint code
bun run lint
cargo clippy --all-features
```

## Project Structure

```
nexastory-desktop/
├── src/                    # Next.js frontend
│   ├── components/         # React components
│   ├── lib/               # Utilities and API
│   └── app/               # Next.js app router
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── llm.rs        # llama.cpp integration
│   │   ├── models.rs     # Data models
│   │   ├── database.rs   # SQLite operations
│   │   ├── enrichment.rs # Prompt enrichment
│   │   └── memory.rs     # Memory optimization
│   └── Cargo.toml
├── .github/workflows/     # CI/CD pipelines
└── scripts/              # Build scripts
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [llama.cpp](https://github.com/ggerganov/llama.cpp) - GGUF inference engine
- [Tauri](https://tauri.app) - Desktop application framework
- [shadcn/ui](https://ui.shadcn.com) - UI components
