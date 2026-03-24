use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactType {
    LoraAdapter,
    FullModel,
    Checkpoint,
}

impl std::fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoraAdapter => write!(f, "lora_adapter"),
            Self::FullModel => write!(f, "full_model"),
            Self::Checkpoint => write!(f, "checkpoint"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_type_display() {
        assert_eq!(ArtifactType::LoraAdapter.to_string(), "lora_adapter");
    }

    #[test]
    fn artifact_type_roundtrip() {
        let artifact_type = ArtifactType::FullModel;
        let json = serde_json::to_string(&artifact_type).unwrap();
        let decoded: ArtifactType = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, artifact_type);
    }
}
