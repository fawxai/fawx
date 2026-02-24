package ai.citros.core

object ActionPolicyNormalizer {
    fun normalizeAppIdentifier(
        contextAppIdentifier: String?,
        fallbackDisplayName: String?
    ): String? {
        val pkg = contextAppIdentifier?.trim()?.lowercase().orEmpty()
        if (pkg.isNotBlank()) return pkg

        val appName = fallbackDisplayName?.trim()?.lowercase().orEmpty()
        if (appName.isNotBlank()) return "app_name:$appName"

        return null
    }

    fun isSamePackageFamily(a: String, b: String): Boolean {
        val left = a.trim().lowercase()
        val right = b.trim().lowercase()
        if (left == right) return true
        return left.startsWith("$right.") || right.startsWith("$left.")
    }

    fun matchesAnyPackage(actual: String, allowed: Set<String>): Boolean {
        val normalizedActual = actual.trim().lowercase()
        return allowed.any { isSamePackageFamily(normalizedActual, it) }
    }
}
