# LAI - Local AI

![LAI](docs/lai.png)

LAI is an opinionated local agent harness on Apple Silicon for developers who are tired of being cucked by the AI overlords.

## Developer Quickstart

1. [Install Rust](https://rust-lang.org/tools/install/) 
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

1. Install dev dependencies, Xcode, and Metal toolchain if not already installed
```
xcode-select --install
xcodebuild -runFirstLaunch
xcodebuild -downloadComponent MetalToolchain

brew update
brew install cmake

```

1. Build
```sh
cargo build --release
```

1. Test
```sh
cargo nextest run
```

1. Download a `mlx` friendly HuggingFace model. This model will be saved under `~/.lai/models/<owner>/<model>/`.
Models may not run if they're not `mlx` compatible. 
You may also need to set a huggingface token for private models on huggingface.

```sh
lai key set huggingface
lai download "DreamFoundries/supergemma4-e4b-abliterated-6bit"
```

1. Test out the model
```sh
lai config model "DreamFoundries/supergemma4-e4b-abliterated-6bit"
lai serve -d -p
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
lai serve -p              # show the desktop cat while serving
lai serve -p -d           # run the server and cat in the background
lai serve --port 8081     # choose a different local port  (default: 8989)
lai kill                  # kill an existing lai serving in the background
```

`lai kill` asks the detached server to stop after its current request finishes. It does not stop a foreground server. A detached server started by an older LAI version must first be stopped using the PID logged at startup.

The desktop cat is available on macOS. Drag it around the screen, or click it to
open chat. The cat displays its reply beside it. Right-click the cat and choose
the shutdown action to stop the server: it looks sad and fades away while the
current request finishes. Chat history lasts for the current pet session.

Configure the pet with the CLI. Relative model paths are resolved from `~/.lai`,
and animation values are clip names in the GLB:

```sh
lai config pet model set my-pet.glb
lai config pet animation set shutdown Sleep
lai config pet animation unset shutdown   # restore the default
lai config pet model unset                # restore the bundled cat
lai config pet show
```

The settings are stored alongside the other settings in `~/.lai/config.toml`.
Omitted settings use the bundled cat and the defaults shown here:

```toml
# [pet]
# model = "my-pet.glb"

[pet.animations]
idle = "1_normal"
waiting = "6_curious"
reply = "2_happy"
error = "3_sad"
shutdown = "3_sad"
```

The server listens only on `127.0.0.1`, with default base URL `http://127.0.0.1:8989/v1`. 

The API exposes `GET /v1/models`, `POST /v1/chat/completions`, and `POST /v1/completions`. 

Set a default system prompt for chat requests, including the desktop pet:

```sh
lai config systemPrompt "You are a friendly and concise assistant."
lai config systemPrompt ""  # clear the default
```

Restart `lai serve` after changing it. An explicit system message in a chat
request takes precedence. The raw completions endpoint uses its supplied prompt.

Use the configured Hugging Face model ID in the `model` field:
```sh
curl http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"DreamFoundries/supergemma4-e4b-abliterated-6bit","messages":[{"role":"user","content":"Hello!"}],"max_tokens":128}'
```

A helper script is in `./script/completion.sh` to test out the flow.
If `jq` is missing, the script installs it using Homebrew.
```sh
./script/completion.sh "Hello!"
```

The script discovers the loaded model automatically. Set `LAI_BASE_URL` if the server uses a different address, for example `LAI_BASE_URL=http://127.0.0.1:8081/v1 ./script/completion.sh "Hello!"`.

### Speculative decoding

For a compatible Gemma 4 model, enable speculative decoding with:

```sh
lai config speculativeDecoding true
```

LAI stores the setting in `~/.lai/config.toml` and offers to download the Gemma 4 E4B assistant if needed. To use a different compatible, unquantized Gemma 4 assistant, supply `--draft-model owner/model` when enabling it. Restart `lai serve` after changing the setting. Disable it with:

```sh
lai config speculativeDecoding false
```

The draft model uses additional memory, and performance depends on the model and workload. The server logs the number of accepted draft tokens. Speculative decoding currently supports greedy generation only: omit `temperature` or set it to `0`; requests specifying a different temperature are rejected. When disabled, the server uses normal decoding.
