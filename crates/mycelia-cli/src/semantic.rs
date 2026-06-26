use std::path::{Path, PathBuf};
use std::thread;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use mycelia_core::{EmbeddingProvider, Error, Result};

pub(crate) const MODEL_ID: &str = "fastembed-5.17.2:BAAI/bge-small-en-v1.5";
const MODEL_DIMENSIONS: usize = 384;
const QUERY_INSTRUCTION: &str = "Represent this sentence for searching relevant passages: ";
/// Hugging Face cache directory name fastembed uses for this model's repo
/// (`Xenova/bge-small-en-v1.5`). Coupled to hf-hub's `models--{org}--{name}`
/// layout; a layout change only yields a false "not cached", which degrades to
/// lexical retrieval rather than producing a wrong answer.
const MODEL_REPO_DIR: &str = "models--Xenova--bge-small-en-v1.5";

pub(crate) struct FastEmbedProvider {
    model: TextEmbedding,
}

impl FastEmbedProvider {
    /// Loads the model from the local cache only, never reaching the network.
    /// Query and serve paths use this so a missing cache degrades to lexical
    /// retrieval instead of triggering an implicit download.
    pub(crate) fn load(database: &Path) -> Result<Self> {
        if !model_is_cached(database) {
            return Err(Error::EmbeddingProvider(format!(
                "embedding model not cached under {}; run `mycelia embed` to download it",
                model_cache_dir(database).display()
            )));
        }
        Self::initialize(database)
    }

    /// Loads the model, downloading it on first use. Reserved for `embed`, the
    /// explicit model-preparation step, so downloads never happen implicitly.
    pub(crate) fn prepare(database: &Path) -> Result<Self> {
        Self::initialize(database)
    }

    fn initialize(database: &Path) -> Result<Self> {
        initialize_onnx_runtime()?;
        let cpu_count = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let options = TextInitOptions::new(EmbeddingModel::BGESmallENV15)
            .with_cache_dir(model_cache_dir(database))
            .with_show_download_progress(true)
            .with_intra_threads(cpu_count);
        let model = TextEmbedding::try_new(options)
            .map_err(|error| Error::EmbeddingProvider(error.to_string()))?;
        Ok(Self { model })
    }
}

#[cfg(not(feature = "semantic-system-ort"))]
fn initialize_onnx_runtime() -> Result<()> {
    Ok(())
}

#[cfg(feature = "semantic-system-ort")]
fn initialize_onnx_runtime() -> Result<()> {
    let path = system_onnxruntime_path().ok_or_else(|| {
        Error::EmbeddingProvider(
            "could not find Homebrew ONNX Runtime; install `onnxruntime` or set ORT_DYLIB_PATH"
                .to_owned(),
        )
    })?;
    ort::init_from(&path)
        .map_err(|error| Error::EmbeddingProvider(error.to_string()))?
        .with_name("mycelia")
        .with_telemetry(false)
        .commit();
    Ok(())
}

#[cfg(feature = "semantic-system-ort")]
fn system_onnxruntime_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ORT_DYLIB_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return Some(path);
    }

    homebrew_prefixes()
        .into_iter()
        .flat_map(|prefix| {
            [
                prefix
                    .join("opt/onnxruntime/lib")
                    .join(onnxruntime_library()),
                prefix.join("lib").join(onnxruntime_library()),
            ]
        })
        .find(|path| path.exists())
}

#[cfg(feature = "semantic-system-ort")]
fn homebrew_prefixes() -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    if let Some(prefix) = std::env::var_os("HOMEBREW_PREFIX")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        prefixes.push(prefix);
    }
    prefixes.push(PathBuf::from("/opt/homebrew"));
    prefixes.push(PathBuf::from("/usr/local"));
    prefixes
}

#[cfg(all(feature = "semantic-system-ort", target_os = "macos"))]
fn onnxruntime_library() -> &'static str {
    "libonnxruntime.dylib"
}

#[cfg(all(
    feature = "semantic-system-ort",
    any(target_os = "linux", target_os = "android")
))]
fn onnxruntime_library() -> &'static str {
    "libonnxruntime.so"
}

#[cfg(all(feature = "semantic-system-ort", target_os = "windows"))]
fn onnxruntime_library() -> &'static str {
    "onnxruntime.dll"
}

#[cfg(all(
    feature = "semantic-system-ort",
    not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "windows"
    ))
))]
fn onnxruntime_library() -> &'static str {
    "libonnxruntime.so"
}

fn model_cache_dir(database: &Path) -> PathBuf {
    database
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("models")
}

/// Reports whether the model's ONNX weights are already present in the local
/// cache, so callers can stay offline. Checks for a `model.onnx` under any
/// downloaded snapshot; a partial cache reads as not cached.
fn model_is_cached(database: &Path) -> bool {
    let snapshots = model_cache_dir(database)
        .join(MODEL_REPO_DIR)
        .join("snapshots");
    let Ok(revisions) = std::fs::read_dir(snapshots) else {
        return false;
    };
    revisions
        .filter_map(std::result::Result::ok)
        .any(|revision| revision.path().join("onnx").join("model.onnx").exists())
}

impl EmbeddingProvider for FastEmbedProvider {
    fn model_id(&self) -> &str {
        MODEL_ID
    }

    fn dimensions(&self) -> usize {
        MODEL_DIMENSIONS
    }

    fn embed_documents(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.model
            .embed(texts, None)
            .map_err(|error| Error::EmbeddingProvider(error.to_string()))
    }

    fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
        let input = format!("{QUERY_INSTRUCTION}{query}");
        self.model
            .embed(vec![input], None)
            .map_err(|error| Error::EmbeddingProvider(error.to_string()))?
            .into_iter()
            .next()
            .ok_or_else(|| Error::EmbeddingProvider("provider returned no query vector".to_owned()))
    }
}
