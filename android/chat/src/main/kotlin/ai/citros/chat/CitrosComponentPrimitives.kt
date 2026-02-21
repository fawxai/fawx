package ai.citros.chat

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

internal data class ButtonColors(
    val containerColor: Color,
    val contentColor: Color,
    val disabledContainerColor: Color,
    val disabledContentColor: Color
)

internal object ButtonDefaults {
    @Composable
    fun buttonColors(
        containerColor: Color = CitrosColorScheme.primary,
        contentColor: Color = contrastOn(containerColor),
        disabledContainerColor: Color = CitrosColorScheme.surfaceContainer,
        disabledContentColor: Color = CitrosColorScheme.onSurfaceVariant
    ): ButtonColors {
        return ButtonColors(
            containerColor = containerColor,
            contentColor = contentColor,
            disabledContainerColor = disabledContainerColor,
            disabledContentColor = disabledContentColor
        )
    }

    @Composable
    fun outlinedButtonColors(
        contentColor: Color = CitrosColorScheme.onSurface,
        disabledContentColor: Color = CitrosColorScheme.onSurfaceVariant
    ): ButtonColors {
        return ButtonColors(
            containerColor = Color.Transparent,
            contentColor = contentColor,
            disabledContainerColor = Color.Transparent,
            disabledContentColor = disabledContentColor
        )
    }
}

@Composable
internal fun Button(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    shape: Shape = RoundedCornerShape(cg(3)),
    colors: ButtonColors = ButtonDefaults.buttonColors(),
    border: androidx.compose.foundation.BorderStroke? = null,
    contentPadding: PaddingValues = PaddingValues(horizontal = cg(4), vertical = cg(2.5f)),
    content: @Composable RowScope.() -> Unit
) {
    val containerColor = if (enabled) colors.containerColor else colors.disabledContainerColor
    val contentColor = if (enabled) colors.contentColor else colors.disabledContentColor

    Box(
        modifier = modifier
            .clip(shape)
            .background(containerColor)
            .then(if (border != null) Modifier.border(border, shape) else Modifier)
            .defaultMinSize(minHeight = cg(11))
            .clickable(
                enabled = enabled,
                indication = null,
                interactionSource = remember { MutableInteractionSource() },
                onClick = onClick
            )
            .padding(contentPadding),
        contentAlignment = Alignment.Center
    ) {
        CompositionLocalProvider(LocalCitrosContentColor provides contentColor) {
            Row(
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
                content = content
            )
        }
    }
}

@Composable
internal fun OutlinedButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    shape: Shape = RoundedCornerShape(cg(3)),
    colors: ButtonColors = ButtonDefaults.outlinedButtonColors(),
    border: androidx.compose.foundation.BorderStroke? = androidx.compose.foundation.BorderStroke(1.dp, CitrosColorScheme.outline),
    contentPadding: PaddingValues = PaddingValues(horizontal = cg(4), vertical = cg(2.5f)),
    content: @Composable RowScope.() -> Unit
) {
    Button(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled,
        shape = shape,
        colors = colors,
        border = border,
        contentPadding = contentPadding,
        content = content
    )
}

@Composable
internal fun TextButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    contentPadding: PaddingValues = PaddingValues(horizontal = cg(2), vertical = cg(1.5f)),
    content: @Composable RowScope.() -> Unit
) {
    Box(
        modifier = modifier
            .defaultMinSize(minHeight = cg(11))
            .clickable(
                enabled = enabled,
                indication = null,
                interactionSource = remember { MutableInteractionSource() },
                onClick = onClick
            )
            .padding(contentPadding),
        contentAlignment = Alignment.Center
    ) {
        CompositionLocalProvider(
            LocalCitrosContentColor provides if (enabled) CitrosColorScheme.primary else CitrosColorScheme.onSurfaceVariant
        ) {
            Row(
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
                content = content
            )
        }
    }
}

@Composable
internal fun Surface(
    modifier: Modifier = Modifier,
    shape: Shape = RoundedCornerShape(0.dp),
    color: Color = Color.Transparent,
    contentColor: Color = Color.Unspecified,
    border: androidx.compose.foundation.BorderStroke? = null,
    tonalElevation: Dp = 0.dp,
    content: @Composable BoxScope.() -> Unit
) {
    Box(
        modifier = modifier
            .clip(shape)
            .background(color)
            .then(if (border != null) Modifier.border(border, shape) else Modifier)
    ) {
        CompositionLocalProvider(
            LocalCitrosContentColor provides if (contentColor == Color.Unspecified) LocalCitrosContentColor.current else contentColor
        ) {
            content()
        }
    }
}

@Composable
internal fun Surface(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    shape: Shape = RoundedCornerShape(0.dp),
    color: Color = Color.Transparent,
    contentColor: Color = Color.Unspecified,
    border: androidx.compose.foundation.BorderStroke? = null,
    tonalElevation: Dp = 0.dp,
    content: @Composable BoxScope.() -> Unit
) {
    Surface(
        modifier = modifier.clickable(
            enabled = enabled,
            indication = null,
            interactionSource = remember { MutableInteractionSource() },
            onClick = onClick
        ),
        shape = shape,
        color = color,
        contentColor = contentColor,
        border = border,
        tonalElevation = tonalElevation,
        content = content
    )
}

@Composable
internal fun HorizontalDivider(
    modifier: Modifier = Modifier,
    color: Color = CitrosColorScheme.outline,
    thickness: Dp = 0.5.dp
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(thickness)
            .background(color)
    )
}

@Composable
internal fun Scaffold(
    modifier: Modifier = Modifier,
    containerColor: Color = Color.Transparent,
    topBar: @Composable () -> Unit = {},
    bottomBar: @Composable () -> Unit = {},
    snackbarHost: @Composable () -> Unit = {},
    content: @Composable (PaddingValues) -> Unit
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(containerColor)
    ) {
        topBar()
        Box(modifier = Modifier.weight(1f)) {
            content(PaddingValues())
        }
        snackbarHost()
        bottomBar()
    }
}

@Composable
internal fun TopAppBar(
    title: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    navigationIcon: @Composable () -> Unit = {},
    actions: @Composable RowScope.() -> Unit = {}
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(cg(12))
            .padding(horizontal = cg(3)),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier.requiredSize(cg(11)),
            contentAlignment = Alignment.Center
        ) {
            navigationIcon()
        }
        Spacer(Modifier.size(cg(2)))
        Box(modifier = Modifier.weight(1f)) { title() }
        Row(
            modifier = Modifier.defaultMinSize(minHeight = cg(11)),
            verticalAlignment = Alignment.CenterVertically,
            content = actions
        )
    }
}

internal data class TextFieldColors(
    val focusedBorderColor: Color,
    val unfocusedBorderColor: Color,
    val disabledBorderColor: Color,
    val focusedContainerColor: Color,
    val unfocusedContainerColor: Color,
    val disabledContainerColor: Color,
    val focusedTextColor: Color,
    val unfocusedTextColor: Color,
    val disabledTextColor: Color,
    val focusedLabelColor: Color,
    val unfocusedLabelColor: Color,
    val disabledLabelColor: Color,
    val cursorColor: Color,
    val focusedPlaceholderColor: Color,
    val unfocusedPlaceholderColor: Color,
    val disabledPlaceholderColor: Color
)

internal object OutlinedTextFieldDefaults {
    @Composable
    fun colors(
        focusedBorderColor: Color = CitrosColorScheme.primary,
        unfocusedBorderColor: Color = CitrosColorScheme.outline,
        disabledBorderColor: Color = CitrosColorScheme.outlineVariant,
        focusedContainerColor: Color = CitrosColorScheme.surface,
        unfocusedContainerColor: Color = CitrosColorScheme.surface,
        disabledContainerColor: Color = CitrosColorScheme.surfaceContainer,
        focusedTextColor: Color = CitrosColorScheme.onSurface,
        unfocusedTextColor: Color = CitrosColorScheme.onSurface,
        disabledTextColor: Color = CitrosColorScheme.onSurfaceVariant,
        focusedLabelColor: Color = CitrosColorScheme.primary,
        unfocusedLabelColor: Color = CitrosColorScheme.onSurfaceVariant,
        disabledLabelColor: Color = CitrosColorScheme.onSurfaceVariant,
        cursorColor: Color = CitrosColorScheme.primary,
        focusedPlaceholderColor: Color = CitrosColorScheme.onSurfaceVariant,
        unfocusedPlaceholderColor: Color = CitrosColorScheme.onSurfaceVariant,
        disabledPlaceholderColor: Color = CitrosColorScheme.onSurfaceVariant,
        focusedIndicatorColor: Color = Color.Transparent,
        unfocusedIndicatorColor: Color = Color.Transparent,
        disabledIndicatorColor: Color = Color.Transparent,
        focusedSupportingTextColor: Color = Color.Unspecified,
        unfocusedSupportingTextColor: Color = Color.Unspecified,
        disabledSupportingTextColor: Color = Color.Unspecified,
        errorBorderColor: Color = Color.Unspecified,
        errorContainerColor: Color = Color.Unspecified,
        errorCursorColor: Color = Color.Unspecified,
        errorLabelColor: Color = Color.Unspecified,
        errorPlaceholderColor: Color = Color.Unspecified,
        errorSupportingTextColor: Color = Color.Unspecified,
        errorTextColor: Color = Color.Unspecified,
        errorTrailingIconColor: Color = Color.Unspecified,
        focusedLeadingIconColor: Color = Color.Unspecified,
        focusedTrailingIconColor: Color = Color.Unspecified,
        unfocusedLeadingIconColor: Color = Color.Unspecified,
        unfocusedTrailingIconColor: Color = Color.Unspecified,
        disabledLeadingIconColor: Color = Color.Unspecified,
        disabledTrailingIconColor: Color = Color.Unspecified
    ): TextFieldColors {
        return TextFieldColors(
            focusedBorderColor = focusedBorderColor,
            unfocusedBorderColor = unfocusedBorderColor,
            disabledBorderColor = disabledBorderColor,
            focusedContainerColor = focusedContainerColor,
            unfocusedContainerColor = unfocusedContainerColor,
            disabledContainerColor = disabledContainerColor,
            focusedTextColor = focusedTextColor,
            unfocusedTextColor = unfocusedTextColor,
            disabledTextColor = disabledTextColor,
            focusedLabelColor = focusedLabelColor,
            unfocusedLabelColor = unfocusedLabelColor,
            disabledLabelColor = disabledLabelColor,
            cursorColor = cursorColor,
            focusedPlaceholderColor = focusedPlaceholderColor,
            unfocusedPlaceholderColor = unfocusedPlaceholderColor,
            disabledPlaceholderColor = disabledPlaceholderColor
        )
    }
}

internal object TextFieldDefaults {
    @Composable
    fun colors(
        focusedBorderColor: Color = Color.Transparent,
        unfocusedBorderColor: Color = Color.Transparent,
        disabledBorderColor: Color = Color.Transparent,
        focusedContainerColor: Color = CitrosColorScheme.surface,
        unfocusedContainerColor: Color = CitrosColorScheme.surface,
        disabledContainerColor: Color = CitrosColorScheme.surfaceContainer,
        focusedTextColor: Color = CitrosColorScheme.onSurface,
        unfocusedTextColor: Color = CitrosColorScheme.onSurface,
        disabledTextColor: Color = CitrosColorScheme.onSurfaceVariant,
        focusedLabelColor: Color = CitrosColorScheme.primary,
        unfocusedLabelColor: Color = CitrosColorScheme.onSurfaceVariant,
        disabledLabelColor: Color = CitrosColorScheme.onSurfaceVariant,
        cursorColor: Color = CitrosColorScheme.primary,
        focusedPlaceholderColor: Color = CitrosColorScheme.onSurfaceVariant,
        unfocusedPlaceholderColor: Color = CitrosColorScheme.onSurfaceVariant,
        disabledPlaceholderColor: Color = CitrosColorScheme.onSurfaceVariant,
        focusedIndicatorColor: Color = Color.Transparent,
        unfocusedIndicatorColor: Color = Color.Transparent,
        disabledIndicatorColor: Color = Color.Transparent,
        errorIndicatorColor: Color = Color.Transparent
    ): TextFieldColors {
        return TextFieldColors(
            focusedBorderColor = focusedBorderColor,
            unfocusedBorderColor = unfocusedBorderColor,
            disabledBorderColor = disabledBorderColor,
            focusedContainerColor = focusedContainerColor,
            unfocusedContainerColor = unfocusedContainerColor,
            disabledContainerColor = disabledContainerColor,
            focusedTextColor = focusedTextColor,
            unfocusedTextColor = unfocusedTextColor,
            disabledTextColor = disabledTextColor,
            focusedLabelColor = focusedLabelColor,
            unfocusedLabelColor = unfocusedLabelColor,
            disabledLabelColor = disabledLabelColor,
            cursorColor = cursorColor,
            focusedPlaceholderColor = focusedPlaceholderColor,
            unfocusedPlaceholderColor = unfocusedPlaceholderColor,
            disabledPlaceholderColor = disabledPlaceholderColor
        )
    }
}

@Composable
internal fun OutlinedTextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    contentDescription: String? = null,
    label: (@Composable () -> Unit)? = null,
    placeholder: (@Composable () -> Unit)? = null,
    trailingIcon: (@Composable () -> Unit)? = null,
    visualTransformation: VisualTransformation = VisualTransformation.None,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    singleLine: Boolean = false,
    maxLines: Int = Int.MAX_VALUE,
    centerSingleLineContentWhenMultiline: Boolean = false,
    textStyle: TextStyle = CitrosTypography.body.copy(
        platformStyle = PlatformTextStyle(includeFontPadding = false)
    ),
    shape: Shape = RoundedCornerShape(cg(3)),
    colors: TextFieldColors = OutlinedTextFieldDefaults.colors()
) {
    CitrosTextFieldBase(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier,
        enabled = enabled,
        contentDescription = contentDescription,
        label = label,
        placeholder = placeholder,
        trailingIcon = trailingIcon,
        visualTransformation = visualTransformation,
        keyboardOptions = keyboardOptions,
        keyboardActions = keyboardActions,
        singleLine = singleLine,
        maxLines = maxLines,
        centerSingleLineContentWhenMultiline = centerSingleLineContentWhenMultiline,
        textStyle = textStyle,
        shape = shape,
        colors = colors,
        outlined = true
    )
}

@Composable
internal fun TextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    contentDescription: String? = null,
    label: (@Composable () -> Unit)? = null,
    placeholder: (@Composable () -> Unit)? = null,
    trailingIcon: (@Composable () -> Unit)? = null,
    visualTransformation: VisualTransformation = VisualTransformation.None,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    singleLine: Boolean = false,
    maxLines: Int = Int.MAX_VALUE,
    centerSingleLineContentWhenMultiline: Boolean = false,
    textStyle: TextStyle = CitrosTypography.body.copy(
        platformStyle = PlatformTextStyle(includeFontPadding = false)
    ),
    shape: Shape = RoundedCornerShape(cg(3)),
    colors: TextFieldColors = TextFieldDefaults.colors()
) {
    CitrosTextFieldBase(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier,
        enabled = enabled,
        contentDescription = contentDescription,
        label = label,
        placeholder = placeholder,
        trailingIcon = trailingIcon,
        visualTransformation = visualTransformation,
        keyboardOptions = keyboardOptions,
        keyboardActions = keyboardActions,
        singleLine = singleLine,
        maxLines = maxLines,
        centerSingleLineContentWhenMultiline = centerSingleLineContentWhenMultiline,
        textStyle = textStyle,
        shape = shape,
        colors = colors,
        outlined = false
    )
}

@Composable
private fun CitrosTextFieldBase(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier,
    enabled: Boolean,
    contentDescription: String?,
    label: (@Composable () -> Unit)?,
    placeholder: (@Composable () -> Unit)?,
    trailingIcon: (@Composable () -> Unit)?,
    visualTransformation: VisualTransformation,
    keyboardOptions: KeyboardOptions,
    keyboardActions: KeyboardActions,
    singleLine: Boolean,
    maxLines: Int,
    centerSingleLineContentWhenMultiline: Boolean,
    textStyle: TextStyle,
    shape: Shape,
    colors: TextFieldColors,
    outlined: Boolean
) {
    var hasFocus by remember { mutableStateOf(false) }
    var visualLineCount by remember { mutableStateOf(1) }
    val containerColor = when {
        !enabled -> colors.disabledContainerColor
        hasFocus -> colors.focusedContainerColor
        else -> colors.unfocusedContainerColor
    }
    val borderColor = when {
        !enabled -> colors.disabledBorderColor
        hasFocus -> colors.focusedBorderColor
        else -> colors.unfocusedBorderColor
    }
    val textColor = when {
        !enabled -> colors.disabledTextColor
        hasFocus -> colors.focusedTextColor
        else -> colors.unfocusedTextColor
    }
    val labelColor = when {
        !enabled -> colors.disabledLabelColor
        hasFocus -> colors.focusedLabelColor
        else -> colors.unfocusedLabelColor
    }
    val placeholderColor = when {
        !enabled -> colors.disabledPlaceholderColor
        hasFocus -> colors.focusedPlaceholderColor
        else -> colors.unfocusedPlaceholderColor
    }

    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(cg(1))) {
        if (label != null) {
            CompositionLocalProvider(LocalCitrosContentColor provides labelColor) {
                label()
            }
        }
        val centerContent = singleLine || (centerSingleLineContentWhenMultiline && visualLineCount <= 1)
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(shape)
                .background(containerColor)
                .then(
                    if (outlined) Modifier.border(
                        width = 1.dp,
                        color = borderColor,
                        shape = shape
                    ) else Modifier
                )
                .defaultMinSize(minHeight = cg(12))
                .padding(horizontal = cg(3), vertical = cg(2))
            ,
            contentAlignment = if (centerContent) Alignment.CenterStart else Alignment.TopStart
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Box(modifier = Modifier.weight(1f)) {
                    BasicTextField(
                        value = value,
                        onValueChange = onValueChange,
                        modifier = Modifier
                            .fillMaxWidth()
                            .then(
                                if (contentDescription != null) {
                                    Modifier.semantics { this.contentDescription = contentDescription }
                                } else {
                                    Modifier
                                }
                            )
                            .onFocusChanged { hasFocus = it.isFocused },
                        enabled = enabled,
                        textStyle = textStyle.copy(color = textColor),
                        visualTransformation = visualTransformation,
                        keyboardOptions = keyboardOptions,
                        keyboardActions = keyboardActions,
                        singleLine = singleLine,
                        maxLines = maxLines,
                        onTextLayout = { visualLineCount = it.lineCount },
                        cursorBrush = SolidColor(colors.cursorColor),
                        decorationBox = { innerTextField ->
                            Box(Modifier.fillMaxWidth()) {
                                if (value.isBlank()) {
                                    CompositionLocalProvider(LocalCitrosContentColor provides placeholderColor) {
                                        placeholder?.invoke()
                                    }
                                }
                                innerTextField()
                            }
                        }
                    )
                }
                if (trailingIcon != null) {
                    Spacer(Modifier.size(cg(1)))
                    trailingIcon()
                }
            }
        }
    }
}

internal data class FilterChipColors(
    val containerColor: Color,
    val labelColor: Color,
    val selectedContainerColor: Color,
    val selectedLabelColor: Color
)

internal object FilterChipDefaults {
    @Composable
    fun filterChipColors(
        containerColor: Color = CitrosColorScheme.surfaceVariant,
        labelColor: Color = CitrosColorScheme.onSurfaceVariant,
        selectedContainerColor: Color = CitrosColorScheme.primary.copy(alpha = 0.18f),
        selectedLabelColor: Color = CitrosColorScheme.primary
    ): FilterChipColors {
        return FilterChipColors(
            containerColor = containerColor,
            labelColor = labelColor,
            selectedContainerColor = selectedContainerColor,
            selectedLabelColor = selectedLabelColor
        )
    }
}

@Composable
internal fun FilterChip(
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    colors: FilterChipColors = FilterChipDefaults.filterChipColors(),
    label: @Composable () -> Unit
) {
    val bg = if (selected) colors.selectedContainerColor else colors.containerColor
    val fg = if (selected) colors.selectedLabelColor else colors.labelColor
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(999.dp))
            .background(bg)
            .clickable(
                enabled = enabled,
                indication = null,
                interactionSource = remember { MutableInteractionSource() },
                onClick = onClick
            )
            .padding(horizontal = cg(3), vertical = cg(2)),
        contentAlignment = Alignment.Center
    ) {
        CompositionLocalProvider(LocalCitrosContentColor provides fg) {
            label()
        }
    }
}

internal data class AssistChipColors(
    val containerColor: Color,
    val labelColor: Color
)

internal object AssistChipDefaults {
    @Composable
    fun assistChipColors(
        containerColor: Color = CitrosColorScheme.surfaceVariant,
        labelColor: Color = CitrosColorScheme.onSurfaceVariant
    ): AssistChipColors {
        return AssistChipColors(containerColor = containerColor, labelColor = labelColor)
    }
}

@Composable
internal fun AssistChip(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    colors: AssistChipColors = AssistChipDefaults.assistChipColors(),
    label: @Composable () -> Unit
) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(999.dp))
            .background(colors.containerColor)
            .clickable(
                enabled = enabled,
                indication = null,
                interactionSource = remember { MutableInteractionSource() },
                onClick = onClick
            )
            .padding(horizontal = cg(3), vertical = cg(1.5f)),
        contentAlignment = Alignment.Center
    ) {
        CompositionLocalProvider(LocalCitrosContentColor provides colors.labelColor) {
            label()
        }
    }
}

internal data class SwitchColors(
    val checkedThumbColor: Color,
    val checkedTrackColor: Color,
    val uncheckedThumbColor: Color,
    val uncheckedTrackColor: Color
)

internal object SwitchDefaults {
    @Composable
    fun colors(
        checkedThumbColor: Color = Color.White,
        checkedTrackColor: Color = CitrosColorScheme.primary,
        checkedBorderColor: Color = Color.Transparent,
        uncheckedThumbColor: Color = Color.White,
        uncheckedTrackColor: Color = CitrosColorScheme.surfaceContainer,
        uncheckedBorderColor: Color = Color.Transparent,
        disabledCheckedThumbColor: Color = checkedThumbColor.copy(alpha = 0.7f),
        disabledCheckedTrackColor: Color = checkedTrackColor.copy(alpha = 0.6f),
        disabledUncheckedThumbColor: Color = uncheckedThumbColor.copy(alpha = 0.7f),
        disabledUncheckedTrackColor: Color = uncheckedTrackColor.copy(alpha = 0.6f)
    ): SwitchColors {
        return SwitchColors(
            checkedThumbColor = checkedThumbColor,
            checkedTrackColor = checkedTrackColor,
            uncheckedThumbColor = uncheckedThumbColor,
            uncheckedTrackColor = uncheckedTrackColor
        )
    }
}

@Composable
internal fun Switch(
    checked: Boolean,
    onCheckedChange: ((Boolean) -> Unit)?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    colors: SwitchColors = SwitchDefaults.colors()
) {
    val trackColor = if (checked) colors.checkedTrackColor else colors.uncheckedTrackColor
    val thumbColor = if (checked) colors.checkedThumbColor else colors.uncheckedThumbColor
    val thumbOffset by animateDpAsState(
        targetValue = if (checked) 20.dp else 2.dp,
        animationSpec = tween(durationMillis = 150),
        label = "citros_switch_thumb_offset"
    )

    Box(
        modifier = modifier
            .size(width = 51.dp, height = 31.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(trackColor)
            .clickable(
                enabled = enabled && onCheckedChange != null,
                indication = null,
                interactionSource = remember { MutableInteractionSource() }
            ) {
                onCheckedChange?.invoke(!checked)
            }
    ) {
        Box(
            modifier = Modifier
                .padding(start = thumbOffset, top = 2.dp)
                .size(27.dp)
                .clip(CircleShape)
                .background(thumbColor)
        )
    }
}

@Composable
internal fun RadioButton(
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true
) {
    Box(
        modifier = modifier
            .size(cg(6))
            .clip(CircleShape)
            .border(1.5.dp, if (selected) CitrosColorScheme.primary else CitrosColorScheme.outline, CircleShape)
            .clickable(
                enabled = enabled,
                indication = null,
                interactionSource = remember { MutableInteractionSource() },
                onClick = onClick
            ),
        contentAlignment = Alignment.Center
    ) {
        if (selected) {
            Box(
                modifier = Modifier
                    .size(cg(2.5f))
                    .clip(CircleShape)
                    .background(CitrosColorScheme.primary)
            )
        }
    }
}

@Composable
internal fun CircularProgressIndicator(
    modifier: Modifier = Modifier,
    color: Color = CitrosColorScheme.primary,
    trackColor: Color = CitrosColorScheme.outline.copy(alpha = 0.35f),
    strokeWidth: Dp = 2.dp
) {
    val transition = rememberInfiniteTransition(label = "citros_progress")
    val rotation by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(animation = tween(durationMillis = 900)),
        label = "citros_progress_rotation"
    )
    androidx.compose.foundation.Canvas(modifier = modifier.rotate(rotation)) {
        val strokePx = strokeWidth.toPx()
        drawArc(
            color = trackColor,
            startAngle = 0f,
            sweepAngle = 360f,
            useCenter = false,
            style = Stroke(width = strokePx, cap = StrokeCap.Round),
            size = Size(size.width, size.height)
        )
        drawArc(
            color = color,
            startAngle = -90f,
            sweepAngle = 110f,
            useCenter = false,
            style = Stroke(width = strokePx, cap = StrokeCap.Round),
            size = Size(size.width, size.height)
        )
    }
}

@Composable
internal fun ModalBottomSheet(
    onDismissRequest: () -> Unit,
    modifier: Modifier = Modifier,
    containerColor: Color = CitrosColorScheme.surface,
    contentColor: Color = CitrosColorScheme.onSurface,
    scrimColor: Color = Color.Black.copy(alpha = 0.45f),
    dragHandle: (@Composable (() -> Unit))? = {
        Box(
            modifier = Modifier
                .padding(top = cg(2), bottom = cg(1))
                .size(width = cg(11), height = 5.dp)
                .background(CitrosColorScheme.outlineVariant, RoundedCornerShape(999.dp))
        )
    },
    content: @Composable ColumnScope.() -> Unit
) {
    Box(modifier = Modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .matchParentSize()
                .background(scrimColor)
                .clickable(
                    indication = null,
                    interactionSource = remember { MutableInteractionSource() },
                    onClick = onDismissRequest
                )
        )
        Column(
            modifier = modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .clip(RoundedCornerShape(topStart = cg(6), topEnd = cg(6)))
                .background(containerColor)
                .navigationBarsPadding()
        ) {
            if (dragHandle != null) {
                Box(
                    modifier = Modifier.fillMaxWidth(),
                    contentAlignment = Alignment.Center
                ) {
                    dragHandle()
                }
            }
            CompositionLocalProvider(LocalCitrosContentColor provides contentColor) {
                content()
            }
        }
    }
}

@Composable
internal fun Snackbar(
    modifier: Modifier = Modifier,
    action: (@Composable () -> Unit)? = null,
    content: @Composable () -> Unit
) {
    val isDark = LocalCitrosIsDark.current
    val surfaces = remember(isDark) { citrosDirectiveSurfaces(isDark) }
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(cg(3)),
        color = surfaces.surface2,
        border = androidx.compose.foundation.BorderStroke(1.dp, surfaces.separatorLight)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = cg(3), vertical = cg(2.5f)),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(cg(2))
        ) {
            Box(modifier = Modifier.weight(1f)) { content() }
            if (action != null) {
                action()
            }
        }
    }
}
