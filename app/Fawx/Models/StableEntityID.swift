import Foundation

/// Deterministic 64-bit FNV-1a IDs are stable across launches, with collision risk negligible for Fawx's workspace/thread key space.
func stableEntityID(prefix: String, value: String) -> String {
    var hash: UInt64 = 0xcbf29ce484222325
    for byte in value.utf8 {
        hash ^= UInt64(byte)
        hash = hash &* 0x100000001b3
    }
    return String(format: "%@-%016llx", prefix, hash)
}
