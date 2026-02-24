package ai.citros.core

fun createConfiguredActionPolicy(): ActionPolicy =
    if (FeatureFlags.actionPolicyEnabled) DefaultActionPolicy() else PermissiveActionPolicy
