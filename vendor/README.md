# Local MLX patches

`safemlx` 0.1.3 and `safemlx-lm` 0.4.1 are vendored from crates.io under their original licenses. The local patches correct packed quantized shapes for 3-, 5-, and 6-bit weights, and use the Gemma 4 shard index to detect absent audio weights and an unquantized per-layer model projection. They preserve strict validation of the weights that the model expects.

Remove these patches when upstream versions include equivalent fixes and have been verified against the model in `tests/mlx_model.rs`.
