use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{sha256_hex, CHECKSUM_MANIFEST, CONFIG_FILE, MODEL_FILE, TOKENIZER_FILE};

const TEST_TOKENIZER_JSON: &str = r#"{
  "version": "1.0",
  "truncation": null,
  "padding": null,
  "added_tokens": [
    {
      "id": 0,
      "content": "[UNK]",
      "single_word": false,
      "lstrip": false,
      "rstrip": false,
      "normalized": false,
      "special": true
    }
  ],
  "normalizer": null,
  "pre_tokenizer": { "type": "Whitespace" },
  "post_processor": null,
  "decoder": null,
  "model": {
    "type": "WordLevel",
    "vocab": {
      "[UNK]": 0,
      "hello": 1,
      "world": 2,
      "memory": 3,
      "search": 4,
      "semantic": 5,
      "auth": 6,
      "decision": 7,
      "oauth": 8,
      "notes": 9
    },
    "unk_token": "[UNK]"
  }
}"#;

pub fn create_test_model_dir(dimensions: usize) -> TempDir {
    create_test_model_dir_with_config(&format!("{{\"hidden_size\": {dimensions}}}"))
}

pub fn create_test_model_dir_with_config(config_contents: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("tempdir");
    write_test_model_config(temp_dir.path(), config_contents);
    temp_dir
}

fn write_test_model_config(model_dir: &Path, config_contents: &str) {
    fs::create_dir_all(model_dir).expect("model dir");
    write_model_file(model_dir, CONFIG_FILE, config_contents);
    write_model_file(model_dir, TOKENIZER_FILE, TEST_TOKENIZER_JSON);
    write_model_file(model_dir, MODEL_FILE, "placeholder weights");
    write_manifest(model_dir);
}

fn write_model_file(model_dir: &Path, file_name: &str, contents: &str) {
    fs::write(model_dir.join(file_name), contents).expect("write model file");
}

fn write_manifest(model_dir: &Path) {
    let mut lines = Vec::new();
    for file_name in [CONFIG_FILE, TOKENIZER_FILE, MODEL_FILE] {
        let bytes = fs::read(model_dir.join(file_name)).expect("read model file");
        let checksum = sha256_hex(&bytes);
        lines.push(format!("{checksum}  {file_name}"));
    }
    fs::write(model_dir.join(CHECKSUM_MANIFEST), lines.join("\n")).expect("manifest");
}
