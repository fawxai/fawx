use crate::dataset::DatasetRef;
use crate::format::ModelFormat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TrainObjective {
    Lora(LoraConfig),
    FullFinetune(FinetuneConfig),
    ContinuedPretrain(PretrainConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraConfig {
    pub base_model: String,
    pub dataset: DatasetRef,
    pub rank: u32,
    pub alpha: u32,
    pub target_modules: Vec<String>,
    pub learning_rate: f64,
    pub epochs: u32,
    pub batch_size: u32,
    pub warmup_steps: u32,
    pub dropout: f64,
    pub max_grad_norm: Option<f64>,
    pub fp16: bool,
    pub bf16: bool,
    pub output_dir: PathBuf,
    pub output_format: ModelFormat,
    #[serde(default)]
    pub extra_params: HashMap<String, serde_json::Value>,
}

impl Default for LoraConfig {
    fn default() -> Self {
        Self {
            base_model: String::new(),
            dataset: DatasetRef::default(),
            rank: 16,
            alpha: 32,
            target_modules: vec!["q_proj".to_owned(), "v_proj".to_owned()],
            learning_rate: 1e-4,
            epochs: 3,
            batch_size: 4,
            warmup_steps: 10,
            dropout: 0.05,
            max_grad_norm: None,
            fp16: true,
            bf16: false,
            output_dir: PathBuf::from("output"),
            output_format: ModelFormat::Safetensors,
            extra_params: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinetuneConfig {
    pub base_model: String,
    pub dataset: DatasetRef,
    pub learning_rate: f64,
    pub epochs: u32,
    pub batch_size: u32,
    pub gradient_accumulation: u32,
    pub warmup_ratio: f64,
    pub dropout: f64,
    pub max_grad_norm: Option<f64>,
    pub fp16: bool,
    pub bf16: bool,
    pub output_dir: PathBuf,
    pub output_format: ModelFormat,
    #[serde(default)]
    pub extra_params: HashMap<String, serde_json::Value>,
}

impl Default for FinetuneConfig {
    fn default() -> Self {
        Self {
            base_model: String::new(),
            dataset: DatasetRef::default(),
            learning_rate: 2e-5,
            epochs: 3,
            batch_size: 2,
            gradient_accumulation: 4,
            warmup_ratio: 0.03,
            dropout: 0.0,
            max_grad_norm: None,
            fp16: true,
            bf16: false,
            output_dir: PathBuf::from("output"),
            output_format: ModelFormat::Safetensors,
            extra_params: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PretrainConfig {
    pub base_model: String,
    pub corpus: DatasetRef,
    pub learning_rate: f64,
    pub max_steps: u64,
    pub batch_size: u32,
    pub context_length: u32,
    pub warmup_steps: u64,
    pub max_grad_norm: Option<f64>,
    pub fp16: bool,
    pub bf16: bool,
    pub output_dir: PathBuf,
    pub output_format: ModelFormat,
    #[serde(default)]
    pub extra_params: HashMap<String, serde_json::Value>,
}

impl Default for PretrainConfig {
    fn default() -> Self {
        Self {
            base_model: String::new(),
            corpus: DatasetRef::default(),
            learning_rate: 1e-5,
            max_steps: 1000,
            batch_size: 4,
            context_length: 4096,
            warmup_steps: 100,
            max_grad_norm: None,
            fp16: true,
            bf16: false,
            output_dir: PathBuf::from("output"),
            output_format: ModelFormat::Safetensors,
            extra_params: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lora_defaults_sensible() {
        let config = LoraConfig::default();
        assert_eq!(config.rank, 16);
        assert_eq!(config.alpha, 32);
        assert_eq!(config.epochs, 3);
        assert!(config.fp16);
    }

    #[test]
    fn train_objective_variant_matches() {
        let objective = TrainObjective::Lora(LoraConfig::default());
        assert!(matches!(objective, TrainObjective::Lora(_)));
    }

    #[test]
    fn configs_roundtrip_serde() {
        let lora = LoraConfig::default();
        let json = serde_json::to_string(&lora).unwrap();
        let _: LoraConfig = serde_json::from_str(&json).unwrap();

        let finetune = FinetuneConfig::default();
        let json = serde_json::to_string(&finetune).unwrap();
        let _: FinetuneConfig = serde_json::from_str(&json).unwrap();

        let pretrain = PretrainConfig::default();
        let json = serde_json::to_string(&pretrain).unwrap();
        let _: PretrainConfig = serde_json::from_str(&json).unwrap();
    }
}
