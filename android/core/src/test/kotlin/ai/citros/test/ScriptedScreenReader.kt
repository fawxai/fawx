package ai.citros.test

import ai.citros.core.ScreenContent

class ScriptedScreenReader(private val screens: List<ScreenContent>) {
    init {
        require(screens.isNotEmpty()) { "screens must not be empty" }
    }

    private var step = 0

    fun nextScreen(): ScreenContent = screens[minOf(step++, screens.lastIndex)]

    fun reset() {
        step = 0
    }
}
