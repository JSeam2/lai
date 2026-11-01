# LAI - Local AI
LAI is an opinionated local agent harness on Apple Silicon for developers who are tired of being cucked by the AI overlords.

## Advanced Quickstart

1. [Install Rust](https://rust-lang.org/tools/install/) 
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

1. Install Xcode and Metal toolchain if not already installed
```
xcode-select --install
xcodebuild -runFirstLaunch
xcodebuild -downloadComponent MetalToolchain
```

2. Build
```sh
cargo build --release
```

3. Test
```sh
cargo nextest run
```

3. Download a `mlx` friendly HuggingFace model. This model will be saved under `~/.lai/models/<owner>/<model>/`.
Models may not run if they're not `mlx` compatible. 
You may also need to set a huggingface token for private models on huggingface.

```sh
lai key set huggingface
lai download "DreamFoundries/supergemma4-e4b-abliterated-6bit"
```

4. Test out the model by setting it and chatting with it
```sh
lai config model "DreamFoundries/supergemma4-e4b-abliterated-6bit"
lai chat
```

## Documentation

### Structure
- MLX for Local Inference
- SQLite for Memory
- Embedding Model for Memory embedding. [We default to EmbeddingGemma2 is used here](https://ai.google.dev/gemma/docs/embeddinggemma)
- Local LLM Model. [We default to SuperGemma4 E4B Abliterated 6bit quantization](https://huggingface.co/DreamFoundries/supergemma4-e4b-abliterated-6bit)


### Logging

LAI writes logs to standard error at `info` level by default. Set `RUST_LOG` to control verbosity, for example `RUST_LOG=lai=debug lai download owner/model` or `RUST_LOG=lai=warn lai download owner/model`.

### Key and Secret Management

LAI uses the Apple Keychain to store secrets. `.env` variables in the repository is only present for the purpose of debugging and development.

```sh
lai key set huggingface
lai key rotate huggingface
lai key unset huggingface
```

### Select a model

After downloading a model, configure which model LAI should use:

```sh
lai config model set "DreamFoundries/supergemma4-e4b-abliterated-6bit"
lai config model update "owner/another-model"
lai config model unset
```

### Serve the selected model

Building the MLX backend requires CMake, Xcode with the Metal toolchain, and an Apple Silicon Mac. Select and download a supported MLX model first, then run:

```sh
lai serve                 # foreground, Ctrl-C to stop
lai serve -d              # background; logs go to ~/.lai/serve.log
lai serve --port 8081     # choose a different local port
```

The server listens only on `127.0.0.1`, with default base URL `http://127.0.0.1:8080/v1`. Detached startup waits for the model to load and reports the process ID; stop it with `kill <PID>`. The API exposes `GET /v1/models`, `POST /v1/chat/completions`, and `POST /v1/completions`. Use the configured Hugging Face model ID in the `model` field:

```sh
curl http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"DreamFoundries/supergemma4-e4b-abliterated-6bit","messages":[{"role":"user","content":"Hello!"}],"max_tokens":128}'
```

This initial API supports text messages, temperature, and token limits, with one completion per request. Streaming responses use the OpenAI event format but are buffered until generation finishes. Unsupported request fields are rejected; tools, multimodal input, and concurrent generation are not yet supported.
