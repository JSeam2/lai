"""Local EmbeddingGemma 2 worker. Communicates via JSON lines on stdin/stdout."""
import contextlib
import json
import sys
from pathlib import Path

# Third-party loader diagnostics must not be mixed with the protocol.
with contextlib.redirect_stdout(sys.stderr):
    import mlx.core as mx
    from transformers import AutoTokenizer
    from mlx_vlm.embedding_loader import load_embedding_model

    path = Path(sys.argv[1])
    config = json.loads((path / "config.json").read_text())
    if config.get("model_type") != "embedding_gemma2":
        raise ValueError("memory model must be EmbeddingGemma 2")
    config["vision_config"] = None
    config["audio_config"] = None
    model = load_embedding_model(path, config=config)
    tokenizer = AutoTokenizer.from_pretrained(path, local_files_only=True)

print(json.dumps({"ready": True}), flush=True)
for line in sys.stdin:
    try:
        request = json.loads(line)
        prefix = {"query": "task: search result | query: ", "document": "title: none | text: "}[request["purpose"]]
        with contextlib.redirect_stdout(sys.stderr):
            inputs = tokenizer([prefix + request["text"]], truncation=True, max_length=8192, return_tensors="np")
            vector = model(**{key: mx.array(value) for key, value in inputs.items()}).text_embeds
            mx.eval(vector)
        print(json.dumps({"vector": vector[0].tolist()}, allow_nan=False), flush=True)
    except Exception:
        print(json.dumps({"error": "embedding inference failed"}), flush=True)
