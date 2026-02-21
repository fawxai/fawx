package ai.citros.chat

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.util.Log

private val FLAVOR_LAUNCHER_ALIAS_SUFFIX = mapOf(
    CitrosFlavor.NONE to ".LauncherNone",
    CitrosFlavor.LEMON to ".LauncherLemon",
    CitrosFlavor.TANGERINE to ".LauncherTangerine",
    CitrosFlavor.LIME to ".LauncherLime",
    CitrosFlavor.BLOOD_ORANGE to ".LauncherBloodOrange",
    CitrosFlavor.GRAPEFRUIT to ".LauncherGrapefruit"
)

/**
 * Foreground icon used by in-app UI previews (e.g. settings rows and floating badge).
 * Home-screen launcher icons are still switched via activity-alias components.
 */
internal fun launcherIconForegroundResForFlavor(flavor: CitrosFlavor): Int = when (flavor) {
    CitrosFlavor.NONE -> R.drawable.ic_launcher_fg_orb_none
    CitrosFlavor.TANGERINE -> R.drawable.ic_launcher_fg_orb_tangerine
    CitrosFlavor.LEMON -> R.drawable.ic_launcher_fg_orb_lemon
    CitrosFlavor.LIME -> R.drawable.ic_launcher_fg_orb_lime
    CitrosFlavor.BLOOD_ORANGE -> R.drawable.ic_launcher_fg_orb_blood_orange
    CitrosFlavor.GRAPEFRUIT -> R.drawable.ic_launcher_fg_orb_grapefruit
}

internal fun syncLauncherIconWithPreferences(context: Context) {
    val prefs = context.getSharedPreferences(ONBOARDING_PREFS, Context.MODE_PRIVATE)
    if (!prefs.getBoolean(PREF_ONBOARDING_COMPLETE, false)) {
        return
    }
    setLauncherIconFlavor(context, readSelectedFlavor(context))
}

internal fun setLauncherIconFlavor(context: Context, flavor: CitrosFlavor) {
    val targetSuffix = FLAVOR_LAUNCHER_ALIAS_SUFFIX[flavor]
    if (targetSuffix == null) {
        Log.e("LauncherIconManager", "No launcher alias mapped for flavor=$flavor")
        return
    }
    val packageName = context.packageName
    val packageManager = context.packageManager

    FLAVOR_LAUNCHER_ALIAS_SUFFIX.values.forEach { suffix ->
        val component = ComponentName(packageName, packageName + suffix)
        val desiredState = if (suffix == targetSuffix) {
            PackageManager.COMPONENT_ENABLED_STATE_ENABLED
        } else {
            PackageManager.COMPONENT_ENABLED_STATE_DISABLED
        }
        try {
            packageManager.setComponentEnabledSetting(
                component,
                desiredState,
                PackageManager.DONT_KILL_APP
            )
        } catch (error: Exception) {
            Log.w("LauncherIconManager", "Failed to update launcher component $suffix", error)
        }
    }
}
