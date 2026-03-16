use crate::ForgeError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelFormat {
    Safetensors,
    Gguf,
    HuggingFace,
    PyTorchBin,
}

impl std::fmt::Display for ModelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Safetensors => write!(f, "safetensors"),
            Self::Gguf => write!(f, "gguf"),
            Self::HuggingFace => write!(f, "huggingface"),
            Self::PyTorchBin => write!(f, "pytorch_bin"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizationConfig {
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    pub output_path: PathBuf,
    pub output_format: ModelFormat,
    pub size_bytes: u64,
    pub quantization: Option<String>,
    pub duration_secs: u64,
}

#[async_trait::async_trait]
pub trait FormatConverter: Send + Sync {
    fn supports(&self, from: &ModelFormat, to: &ModelFormat) -> bool;

    async fn convert(
        &self,
        input_path: &Path,
        output_path: &Path,
        from: &ModelFormat,
        to: &ModelFormat,
        quantization: Option<&QuantizationConfig>,
    ) -> Result<ConversionResult, ForgeError>;
}

/// Mock converter for testing.
pub struct MockConverter {
    supported_from: ModelFormat,
    supported_to: ModelFormat,
}

impl MockConverter {
    pub fn new(from: ModelFormat, to: ModelFormat) -> Self {
        Self {
            supported_from: from,
            supported_to: to,
        }
    }
}

#[async_trait::async_trait]
impl FormatConverter for MockConverter {
    fn supports(&self, from: &ModelFormat, to: &ModelFormat) -> bool {
        *from == self.supported_from && *to == self.supported_to
    }

    async fn convert(
        &self,
        _input_path: &Path,
        output_path: &Path,
        _from: &ModelFormat,
        to: &ModelFormat,
        quantization: Option<&QuantizationConfig>,
    ) -> Result<ConversionResult, ForgeError> {
        Ok(ConversionResult {
            output_path: output_path.to_path_buf(),
            output_format: to.clone(),
            size_bytes: 1024,
            quantization: quantization.map(|config| config.method.clone()),
            duration_secs: 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_format_display() {
        assert_eq!(ModelFormat::Safetensors.to_string(), "safetensors");
        assert_eq!(ModelFormat::Gguf.to_string(), "gguf");
    }

    #[test]
    fn mock_converter_supports() {
        let converter = MockConverter::new(ModelFormat::Safetensors, ModelFormat::Gguf);
        assert!(converter.supports(&ModelFormat::Safetensors, &ModelFormat::Gguf));
        assert!(!converter.supports(&ModelFormat::Gguf, &ModelFormat::Safetensors));
    }

    #[tokio::test]
    async fn mock_converter_converts() {
        let converter = MockConverter::new(ModelFormat::Safetensors, ModelFormat::Gguf);
        let result = converter
            .convert(
                Path::new("input.safetensors"),
                Path::new("output.gguf"),
                &ModelFormat::Safetensors,
                &ModelFormat::Gguf,
                Some(&QuantizationConfig {
                    method: "Q4_K_M".to_owned(),
                }),
            )
            .await
            .unwrap();
        assert_eq!(result.output_format, ModelFormat::Gguf);
        assert_eq!(result.quantization.as_deref(), Some("Q4_K_M"));
    }
}
