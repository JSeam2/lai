//! Persistent local MLX embedding worker.
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use anyhow::{Context, ensure};
use serde::Deserialize;

pub enum EmbeddingPurpose {
    Query,
    Document,
}

pub struct EmbeddingModel {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

#[derive(Deserialize)]
struct Response {
    ready: Option<bool>,
    vector: Option<Vec<f32>>,
    error: Option<String>,
}

impl EmbeddingModel {
    pub fn start(python: &Path, model: &Path) -> anyhow::Result<Self> {
        let mut child = Command::new(python)
            .arg("-u")
            .arg("-c")
            .arg(include_str!("embedding.py"))
            .arg(model)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context(
                "cannot start the memory embedding runtime; see the memory setup instructions",
            )?;
        let input = child
            .stdin
            .take()
            .context("cannot open embedding worker input")?;
        let output = BufReader::new(
            child
                .stdout
                .take()
                .context("cannot open embedding worker output")?,
        );
        let mut worker = Self {
            child,
            input,
            output,
        };
        ensure!(
            worker.read_response()?.ready == Some(true),
            "memory embedding worker did not start"
        );
        Ok(worker)
    }

    pub fn embed(&mut self, text: &str, purpose: EmbeddingPurpose) -> anyhow::Result<Vec<f32>> {
        let purpose = match purpose {
            EmbeddingPurpose::Query => "query",
            EmbeddingPurpose::Document => "document",
        };
        serde_json::to_writer(
            &mut self.input,
            &serde_json::json!({"text":text,"purpose":purpose}),
        )?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let response = self.read_response()?;
        ensure!(
            response.error.is_none(),
            "memory embedding inference failed"
        );
        let vector = response
            .vector
            .context("embedding worker returned no vector")?;
        ensure!(
            vector.len() == 768
                && vector.iter().all(|v| v.is_finite())
                && vector.iter().any(|v| *v != 0.0),
            "embedding worker returned an invalid vector"
        );
        Ok(vector)
    }

    fn read_response(&mut self) -> anyhow::Result<Response> {
        let mut line = String::new();
        ensure!(
            self.output.read_line(&mut line)? > 0,
            "memory embedding worker stopped; check the Python and MLX setup"
        );
        serde_json::from_str(&line).context("memory embedding worker returned an invalid response")
    }
}

impl Drop for EmbeddingModel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
