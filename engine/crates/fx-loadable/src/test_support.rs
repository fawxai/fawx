use std::fs;
use std::io;
use std::path::Path;

pub fn test_manifest_toml(name: &str) -> String {
    versioned_manifest_toml(name, "1.0.0")
}

pub fn versioned_manifest_toml(name: &str, version: &str) -> String {
    format!(
        r#"name = "{name}"
version = "{version}"
description = "{name} skill"
author = "Test"
api_version = "host_api_v1"
entry_point = "run"
"#
    )
}

pub fn invocable_wasm_bytes() -> Vec<u8> {
    let wat = r#"
        (module
            (import "host_api_v1" "log" (func $log (param i32 i32 i32)))
            (import "host_api_v1" "kv_get" (func $kv_get (param i32 i32) (result i32)))
            (import "host_api_v1" "kv_set" (func $kv_set (param i32 i32 i32 i32)))
            (import "host_api_v1" "get_input" (func $get_input (result i32)))
            (import "host_api_v1" "set_output" (func $set_output (param i32 i32)))
            (memory (export "memory") 1)
            (func (export "run")
                (i32.store8 (i32.const 0) (i32.const 111))
                (i32.store8 (i32.const 1) (i32.const 107))
                (call $set_output (i32.const 0) (i32.const 2))
            )
        )
    "#;
    wat.as_bytes().to_vec()
}

pub fn write_test_skill(skills_dir: &Path, name: &str) -> io::Result<()> {
    write_versioned_test_skill(skills_dir, name, "1.0.0")
}

pub fn write_versioned_test_skill(skills_dir: &Path, name: &str, version: &str) -> io::Result<()> {
    let skill_dir = skills_dir.join(name);
    fs::create_dir_all(&skill_dir)?;
    fs::write(
        skill_dir.join("manifest.toml"),
        versioned_manifest_toml(name, version),
    )?;
    fs::write(
        skill_dir.join(format!("{name}.wasm")),
        invocable_wasm_bytes(),
    )?;
    Ok(())
}
