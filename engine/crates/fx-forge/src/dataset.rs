use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DatasetRef {
    Local {
        path: PathBuf,
        format: DatasetFormat,
    },
    Remote {
        url: String,
        format: DatasetFormat,
    },
}

impl Default for DatasetRef {
    fn default() -> Self {
        Self::Local {
            path: PathBuf::from("<unset>"),
            format: DatasetFormat::OpenAiJsonl,
        }
    }
}

impl DatasetRef {
    pub fn format(&self) -> &DatasetFormat {
        match self {
            Self::Local { format, .. } => format,
            Self::Remote { format, .. } => format,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DatasetFormat {
    OpenAiJsonl,
    AlpacaJsonl,
    DpoJsonl,
    RawJson,
    PlainText,
    Jsonl,
    Parquet,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_ref_default() {
        let dataset = DatasetRef::default();
        assert_eq!(dataset.format(), &DatasetFormat::OpenAiJsonl);
        match dataset {
            DatasetRef::Local { path, .. } => assert_eq!(path, PathBuf::from("<unset>")),
            DatasetRef::Remote { .. } => panic!("default dataset should be local"),
        }
    }

    #[test]
    fn dataset_ref_format() {
        let dataset = DatasetRef::Remote {
            url: "https://example.com/data.jsonl".to_owned(),
            format: DatasetFormat::DpoJsonl,
        };
        assert_eq!(dataset.format(), &DatasetFormat::DpoJsonl);
    }

    #[test]
    fn dataset_format_roundtrip() {
        let format = DatasetFormat::AlpacaJsonl;
        let json = serde_json::to_string(&format).unwrap();
        let decoded: DatasetFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, format);
    }
}
