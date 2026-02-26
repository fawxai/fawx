package ai.citros.core

/**
 * Egress allowlist snapshot used by policy checks.
 *
 * Only snapshots with [signatureVerified] = true are trusted for outbound access.
 */
data class EgressAllowlistSnapshot(
    val hosts: Set<String>,
    val version: String,
    val signatureVerified: Boolean,
    val appliedAtMs: Long
)

/**
 * Supplies signed host snapshots for outbound tool policy.
 *
 * Production wiring should inject a provider backed by signed config updates.
 * If signature verification fails, policy intentionally fails closed.
 */
interface EgressAllowlistProvider {
    fun currentSnapshot(): EgressAllowlistSnapshot
    fun currentHosts(): Set<String> = currentSnapshot().hosts
}

/**
 * Bootstrap provider for startup-safe defaults.
 *
 * Returns an unsigned empty snapshot, so all egress requests are denied until a
 * signed provider is injected.
 */
object EmptyDenyEgressAllowlistProvider : EgressAllowlistProvider {
    override fun currentSnapshot(): EgressAllowlistSnapshot = EgressAllowlistSnapshot(
        hosts = emptySet(),
        version = "bootstrap:none",
        signatureVerified = false,
        appliedAtMs = 0L
    )
}
