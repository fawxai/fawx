pub(crate) fn stable_entity_id(prefix: &str, value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{prefix}-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::stable_entity_id;

    #[test]
    fn stable_entity_id_is_deterministic() {
        assert_eq!(
            stable_entity_id("workspace", "/tmp/demo"),
            stable_entity_id("workspace", "/tmp/demo")
        );
    }
}
