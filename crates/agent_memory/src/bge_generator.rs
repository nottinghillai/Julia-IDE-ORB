//! BGE (BAAI General Embedding) model implementation
//! 
//! This module provides embedding generation using the BGE-small-en-v1.5 model.
//! The model is downloaded from HuggingFace on first use and cached locally.

#[cfg(feature = "embeddings")]
use crate::embedding::{Embedding, EmbeddingGenerator, EmbeddingModel};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(feature = "embeddings")]
use candle_core::{Device, Tensor, DType};
#[cfg(feature = "embeddings")]
use candle_transformers::models::bert::{BertModel, Config};
#[cfg(feature = "embeddings")]
use candle_nn::VarBuilder;
#[cfg(feature = "embeddings")]
use tokenizers::Tokenizer;
#[cfg(feature = "embeddings")]
use safetensors::SafeTensors;
#[cfg(feature = "embeddings")]
use serde_json::Value;

/// BGE embedding generator
/// 
/// This generator uses the BGE-small-en-v1.5 model to create embeddings.
/// Models are downloaded and cached in the user's data directory.
pub struct BgeEmbeddingGenerator {
    #[cfg(feature = "embeddings")]
    model: Arc<Mutex<Option<BgeModelState>>>,
    model_dir: PathBuf,
    #[cfg(feature = "embeddings")]
    http_client: Option<Arc<dyn http_client::HttpClient>>,
    #[cfg(feature = "embeddings")]
    fs: Option<Arc<dyn fs::Fs>>,
}

#[cfg(feature = "embeddings")]
struct BgeModelState {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl BgeEmbeddingGenerator {
    /// Create a new BGE embedding generator
    pub fn new(model_dir: Option<PathBuf>) -> Self {
        let model_dir = model_dir.unwrap_or_else(|| {
            paths::data_dir().join("models").join("bge-small-en-v1.5")
        });

        Self {
            #[cfg(feature = "embeddings")]
            model: Arc::new(Mutex::new(None)),
            model_dir,
            #[cfg(feature = "embeddings")]
            http_client: None,
            #[cfg(feature = "embeddings")]
            fs: None,
        }
    }

    /// Create a new BGE embedding generator with HTTP client and file system
    #[cfg(feature = "embeddings")]
    pub fn with_resources(
        model_dir: Option<PathBuf>,
        http_client: Option<Arc<dyn http_client::HttpClient>>,
        fs: Option<Arc<dyn fs::Fs>>,
    ) -> Self {
        let model_dir = model_dir.unwrap_or_else(|| {
            paths::data_dir().join("models").join("bge-small-en-v1.5")
        });

        Self {
            model: Arc::new(Mutex::new(None)),
            model_dir,
            http_client,
            fs,
        }
    }

    #[cfg(feature = "embeddings")]
    async fn ensure_model_loaded(&self) -> Result<()> {
        let mut model_state = self.model.lock().await;
        if model_state.is_some() {
            return Ok(());
        }

        log::info!("Loading BGE embedding model from {:?}", self.model_dir);
        
        // Ensure model directory exists
        if let Some(fs) = &self.fs {
            fs.create_dir(&self.model_dir).await?;
        } else {
            std::fs::create_dir_all(&self.model_dir)?;
        }

        // Download model files if needed
        self.download_model_files().await?;

        // Load model files
        let config_path = self.model_dir.join("config.json");
        let tokenizer_path = self.model_dir.join("tokenizer.json");
        let model_path = self.model_dir.join("model.safetensors");

        // Check if files exist
        let files_exist = if let Some(fs) = &self.fs {
            fs.is_file(&config_path).await
                && fs.is_file(&tokenizer_path).await
                && fs.is_file(&model_path).await
        } else {
            config_path.exists() && tokenizer_path.exists() && model_path.exists()
        };

        if !files_exist {
            log::warn!(
                "BGE model files not found at {:?}. Please download manually or provide HTTP client.",
                self.model_dir
            );
            return Ok(());
        }

        // Load config
        let config_str = if let Some(fs) = &self.fs {
            fs.load(&config_path).await?
        } else {
            std::fs::read_to_string(&config_path)?
        };
        // Parse config - Config implements Deserialize
        let config: Config = serde_json::from_str(&config_str)
            .context("Failed to parse BERT config")?;

        // Load tokenizer
        let tokenizer_bytes = if let Some(fs) = &self.fs {
            fs.load_bytes(&tokenizer_path).await?
        } else {
            std::fs::read(&tokenizer_path)?
        };
        let tokenizer = Tokenizer::from_bytes(tokenizer_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        // Load model weights
        let model_bytes = if let Some(fs) = &self.fs {
            fs.load_bytes(&model_path).await?
        } else {
            std::fs::read(&model_path)?
        };
        let device = Device::Cpu;
        
        // Create VarBuilder from safetensors bytes
        // Use from_slice_safetensors which takes the raw bytes
        let vb = VarBuilder::from_slice_safetensors(&model_bytes, DType::F32, &device)
            .context("Failed to create VarBuilder from safetensors")?;
        
        // Initialize model
        let model = BertModel::load(vb, &config)
            .context("Failed to load BERT model")?;

        // Store the loaded model state
        *model_state = Some(BgeModelState {
            model,
            tokenizer,
            device,
        });

        log::info!("BGE model loaded successfully");
        Ok(())
    }

    #[cfg(feature = "embeddings")]
    async fn download_model_files(&self) -> Result<()> {
        // Only download if we have HTTP client
        let Some(http_client) = &self.http_client else {
            return Ok(()); // No HTTP client, skip download
        };

        let base_url = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main";
        let files = [
            ("config.json", "config.json"),
            ("tokenizer.json", "tokenizer.json"),
            ("model.safetensors", "model.safetensors"),
        ];

        for (filename, local_name) in &files {
            let file_path = self.model_dir.join(local_name);
            
            // Skip if file already exists
            let exists = if let Some(fs) = &self.fs {
                fs.is_file(&file_path).await
            } else {
                file_path.exists()
            };
            if exists {
                log::debug!("Model file {} already exists, skipping download", local_name);
                continue;
            }

            let url = format!("{}/{}", base_url, filename);
            log::info!("Downloading {} from HuggingFace...", filename);

            let mut response = http_client
                .get(&url, http_client::AsyncBody::default(), true)
                .await
                .with_context(|| format!("Failed to download {}", filename))?;

            anyhow::ensure!(
                response.status().is_success(),
                "Download failed with status {} for {}",
                response.status(),
                filename
            );

            // Read response body
            let mut bytes = Vec::new();
            use futures::io::AsyncReadExt;
            let mut body = response.body_mut();
            body.read_to_end(&mut bytes).await?;

            // Write to file
            if let Some(fs) = &self.fs {
                fs.write(&file_path, &bytes).await?;
            } else {
                std::fs::write(&file_path, bytes)?;
            }

            log::info!("Downloaded {} successfully", filename);
        }

        Ok(())
    }

    #[cfg(not(feature = "embeddings"))]
    async fn ensure_model_loaded(&self) -> Result<()> {
        // Feature not enabled
        Ok(())
    }

    #[cfg(feature = "embeddings")]
    async fn generate_internal(&self, text: &str) -> Result<Embedding> {
        self.ensure_model_loaded().await?;
        
        let model_state = self.model.lock().await;
        let Some(state) = model_state.as_ref() else {
            // Fallback to placeholder if model not available
            let dimension = EmbeddingModel::BgeSmallEnV15.dimension();
            return Ok(Embedding::new(vec![0.0f32; dimension], EmbeddingModel::BgeSmallEnV15)?);
        };

        // Tokenize text
        let tokens = state.tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization error: {}", e))?;
        let token_ids = tokens.get_ids();
        
        // Convert to tensor
        let token_ids_tensor = Tensor::new(
            token_ids,
            &state.device,
        )?.unsqueeze(0)?; // Add batch dimension

        // Create attention mask (all ones for now - all tokens are valid)
        let seq_len = token_ids.len();
        let attention_mask = Tensor::ones((1, seq_len), DType::U8, &state.device)?;

        // Run through model
        // BertModel::forward takes: input_ids, attention_mask, token_type_ids
        // Returns Tensor directly (last hidden state)
        // Shape: [batch_size, seq_len, hidden_size]
        let hidden_states = state.model.forward(&token_ids_tensor, &attention_mask, None)?;
        let (batch_size, seq_len, hidden_size) = hidden_states.dims3()?;
        
        // Mean pool over sequence length
        let embedding = hidden_states
            .sum_keepdim(1)? // Sum over seq_len
            .squeeze(1)? // Remove seq_len dimension
            .broadcast_div(&Tensor::new(&[seq_len as f32], &state.device)?.unsqueeze(0)?)?;
        
        // Extract as Vec<f32>
        let embedding_vec: Vec<f32> = embedding.to_vec1()?;
        
        // Ensure correct dimension
        if embedding_vec.len() != EmbeddingModel::BgeSmallEnV15.dimension() {
            anyhow::bail!(
                "Embedding dimension mismatch: expected {}, got {}",
                EmbeddingModel::BgeSmallEnV15.dimension(),
                embedding_vec.len()
            );
        }

        Embedding::new(embedding_vec, EmbeddingModel::BgeSmallEnV15)
    }

    #[cfg(not(feature = "embeddings"))]
    async fn generate_internal(&self, _text: &str) -> Result<Embedding> {
        // Feature not enabled - return placeholder
        let dimension = EmbeddingModel::BgeSmallEnV15.dimension();
        Ok(Embedding::new(vec![0.0f32; dimension], EmbeddingModel::BgeSmallEnV15)?)
    }
}

#[async_trait::async_trait]
impl EmbeddingGenerator for BgeEmbeddingGenerator {
    async fn generate(&self, text: &str, model: EmbeddingModel) -> Result<Embedding> {
        // For now, only support BGE model
        if model != EmbeddingModel::BgeSmallEnV15 {
            anyhow::bail!("BgeEmbeddingGenerator only supports BgeSmallEnV15 model");
        }

        let mut embedding = self.generate_internal(text).await?;
        embedding.normalize();
        Ok(embedding)
    }

    async fn generate_batch(
        &self,
        texts: &[String],
        model: EmbeddingModel,
    ) -> Result<Vec<Embedding>> {
        if model != EmbeddingModel::BgeSmallEnV15 {
            anyhow::bail!("BgeEmbeddingGenerator only supports BgeSmallEnV15 model");
        }

        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            let mut embedding = self.generate_internal(text).await?;
            embedding.normalize();
            embeddings.push(embedding);
        }
        Ok(embeddings)
    }
}

