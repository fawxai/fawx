use crate::dataset::DatasetRef;
use crate::format::ModelFormat;
use crate::ForgeError;
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

impl TrainObjective {
    pub fn validate(&self) -> Result<(), ForgeError> {
        match self {
            Self::Lora(config) => config.validate(),
            Self::FullFinetune(config) => config.validate(),
            Self::ContinuedPretrain(config) => config.validate(),
        }
    }
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

impl LoraConfig {
    pub fn validate(&self) -> Result<(), ForgeError> {
        validate_base_model(&self.base_model)?;
        validate_positive_u32(self.rank, "rank")?;
        validate_positive_f64(self.learning_rate, "learning_rate")?;
        validate_positive_u32(self.epochs, "epochs")?;
        validate_positive_u32(self.batch_size, "batch_size")?;
        validate_precision_flags(self.fp16, self.bf16)
    }
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

impl FinetuneConfig {
    pub fn validate(&self) -> Result<(), ForgeError> {
        validate_base_model(&self.base_model)?;
        validate_positive_f64(self.learning_rate, "learning_rate")?;
        validate_positive_u32(self.epochs, "epochs")?;
        validate_positive_u32(self.batch_size, "batch_size")?;
        validate_positive_u32(self.gradient_accumulation, "gradient_accumulation")?;
        validate_precision_flags(self.fp16, self.bf16)
    }
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

impl PretrainConfig {
    pub fn validate(&self) -> Result<(), ForgeError> {
        validate_base_model(&self.base_model)?;
        validate_positive_f64(self.learning_rate, "learning_rate")?;
        validate_positive_u64(self.max_steps, "max_steps")?;
        validate_positive_u32(self.batch_size, "batch_size")?;
        validate_positive_u32(self.context_length, "context_length")?;
        validate_precision_flags(self.fp16, self.bf16)
    }
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

fn validate_base_model(base_model: &str) -> Result<(), ForgeError> {
    if base_model.trim().is_empty() {
        return Err(invalid_config("base_model is empty"));
    }
    Ok(())
}

fn validate_positive_u32(value: u32, field: &str) -> Result<(), ForgeError> {
    if value == 0 {
        return Err(invalid_config(format!("{field} must be > 0")));
    }
    Ok(())
}

fn validate_positive_u64(value: u64, field: &str) -> Result<(), ForgeError> {
    if value == 0 {
        return Err(invalid_config(format!("{field} must be > 0")));
    }
    Ok(())
}

fn validate_positive_f64(value: f64, field: &str) -> Result<(), ForgeError> {
    if value <= 0.0 {
        return Err(invalid_config(format!("{field} must be > 0")));
    }
    Ok(())
}

fn validate_precision_flags(fp16: bool, bf16: bool) -> Result<(), ForgeError> {
    if fp16 && bf16 {
        return Err(invalid_config("fp16 and bf16 are mutually exclusive"));
    }
    Ok(())
}

fn invalid_config(message: impl Into<String>) -> ForgeError {
    ForgeError::InvalidConfig(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_lora_config() -> LoraConfig {
        LoraConfig {
            base_model: "meta-llama/Llama-3.1-8B".to_owned(),
            ..LoraConfig::default()
        }
    }

    fn valid_finetune_config() -> FinetuneConfig {
        FinetuneConfig {
            base_model: "meta-llama/Llama-3.1-8B".to_owned(),
            ..FinetuneConfig::default()
        }
    }

    fn valid_pretrain_config() -> PretrainConfig {
        PretrainConfig {
            base_model: "meta-llama/Llama-3.1-8B".to_owned(),
            ..PretrainConfig::default()
        }
    }

    #[test]
    fn lora_defaults_sensible() {
        let config = LoraConfig::default();
        assert_eq!(config.rank, 16);
        assert_eq!(config.alpha, 32);
        assert_eq!(config.epochs, 3);
        assert!(config.fp16);
    }

    #[test]
    fn lora_validate_rejects_empty_base_model() {
        let error = LoraConfig::default().validate().unwrap_err();
        assert!(error.to_string().contains("base_model is empty"));
    }

    #[test]
    fn lora_validate_rejects_zero_rank() {
        let mut config = valid_lora_config();
        config.rank = 0;
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("rank must be > 0"));
    }

    #[test]
    fn lora_validate_rejects_conflicting_precision() {
        let mut config = valid_lora_config();
        config.bf16 = true;
        let error = config.validate().unwrap_err();
        assert!(error
            .to_string()
            .contains("fp16 and bf16 are mutually exclusive"));
    }

    #[test]
    fn finetune_validate_rejects_zero_epochs() {
        let mut config = valid_finetune_config();
        config.epochs = 0;
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("epochs must be > 0"));
    }

    #[test]
    fn pretrain_validate_rejects_zero_max_steps() {
        let mut config = valid_pretrain_config();
        config.max_steps = 0;
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("max_steps must be > 0"));
    }

    #[test]
    fn valid_configs_pass_validation() {
        valid_lora_config().validate().unwrap();
        valid_finetune_config().validate().unwrap();
        valid_pretrain_config().validate().unwrap();
        TrainObjective::Lora(valid_lora_config())
            .validate()
            .unwrap();
    }

    #[test]
    fn train_objective_variant_matches() {
        let objective = TrainObjective::Lora(valid_lora_config());
        assert!(matches!(objective, TrainObjective::Lora(_)));
    }

    #[test]
    fn configs_roundtrip_serde() {
        let lora = valid_lora_config();
        let json = serde_json::to_string(&lora).unwrap();
        let _: LoraConfig = serde_json::from_str(&json).unwrap();

        let finetune = valid_finetune_config();
        let json = serde_json::to_string(&finetune).unwrap();
        let _: FinetuneConfig = serde_json::from_str(&json).unwrap();

        let pretrain = valid_pretrain_config();
        let json = serde_json::to_string(&pretrain).unwrap();
        let _: PretrainConfig = serde_json::from_str(&json).unwrap();
    }
}
