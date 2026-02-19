package ai.citros.chat

import android.content.Context
import android.graphics.Outline
import android.graphics.PixelFormat
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.opengl.Matrix
import android.os.SystemClock
import android.view.View
import android.view.ViewOutlineProvider
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.viewinterop.AndroidView
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import java.nio.ShortBuffer
import kotlin.math.acos
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.random.Random

@Composable
internal fun CitrosHeroShaderSphere(
    flavor: CitrosFlavor,
    modifier: Modifier = Modifier,
    particleSizeScale: Float = 1f,
    isDark: Boolean? = null,
    clipCircle: Boolean = false
) {
    val resolvedIsDark = isDark ?: LocalCitrosIsDark.current
    AndroidView(
        modifier = modifier,
        factory = { context ->
            CitrosHeroGLSurfaceView(context).apply {
                setFlavor(flavor)
                setParticleSizeScale(particleSizeScale)
                setThemeIsDark(resolvedIsDark)
                setClipCircle(clipCircle)
            }
        },
        update = { view ->
            view.setFlavor(flavor)
            view.setParticleSizeScale(particleSizeScale)
            view.setThemeIsDark(resolvedIsDark)
            view.setClipCircle(clipCircle)
        }
    )
}

private class CitrosHeroGLSurfaceView(context: Context) : GLSurfaceView(context) {
    private val heroRenderer = CitrosHeroRenderer()
    private var clipCircleEnabled: Boolean = false
    private val circleOutlineProvider = object : ViewOutlineProvider() {
        override fun getOutline(view: View, outline: Outline) {
            val size = minOf(view.width, view.height)
            val left = (view.width - size) / 2
            val top = (view.height - size) / 2
            outline.setOval(left, top, left + size, top + size)
        }
    }

    init {
        setEGLContextClientVersion(2)
        setEGLConfigChooser(8, 8, 8, 8, 16, 0)
        holder.setFormat(PixelFormat.TRANSLUCENT)
        setZOrderOnTop(false)
        isClickable = false
        isLongClickable = false
        isFocusable = false
        isFocusableInTouchMode = false
        setOnTouchListener { _, _ -> false }
        preserveEGLContextOnPause = true
        setRenderer(heroRenderer)
        renderMode = RENDERMODE_CONTINUOUSLY
    }

    fun setFlavor(flavor: CitrosFlavor) {
        queueEvent {
            heroRenderer.setFlavor(flavor)
        }
    }

    fun setParticleSizeScale(scale: Float) {
        queueEvent {
            heroRenderer.setParticleSizeScale(scale)
        }
    }

    fun setThemeIsDark(isDark: Boolean) {
        queueEvent {
            heroRenderer.setThemeIsDark(isDark)
        }
    }

    fun setClipCircle(enabled: Boolean) {
        if (clipCircleEnabled == enabled) return
        clipCircleEnabled = enabled
        // Renderer state is applied on the GL thread; clipToOutline is applied immediately on UI thread.
        // A one-frame visual mismatch is acceptable and avoids blocking UI on a GL-thread round trip.
        queueEvent {
            heroRenderer.setClipCircle(enabled)
        }
        if (enabled) {
            outlineProvider = circleOutlineProvider
            clipToOutline = true
        } else {
            outlineProvider = ViewOutlineProvider.BOUNDS
            clipToOutline = false
        }
        invalidateOutline()
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        onResume()
    }

    override fun onDetachedFromWindow() {
        onPause()
        super.onDetachedFromWindow()
    }
}

private class CitrosHeroRenderer : GLSurfaceView.Renderer {
    private var viewportWidth = 1
    private var viewportHeight = 1
    private var startNanos = 0L

    private var sphereProgram: SphereProgram? = null
    private var lineProgram: LineProgram? = null
    private var pointProgram: PointProgram? = null

    private var sphereMesh: SphereMesh? = null
    private var wireframeEdges: EdgeMesh? = null
    private var particleField: ParticleField? = null
    private var ringMesh: RingMesh? = null

    @Volatile
    private var currentFlavor: CitrosFlavor = CitrosFlavor.TANGERINE

    @Volatile
    private var isDarkTheme: Boolean = false

    @Volatile
    private var palette: HeroPalette = HeroPalette.fromFlavor(
        flavor = currentFlavor,
        isDark = isDarkTheme
    )

    @Volatile
    private var particleSizeScale: Float = 1f

    @Volatile
    private var clipCircle: Boolean = false

    private val projectionMatrix = FloatArray(16)
    private val viewMatrix = FloatArray(16)
    private val modelMatrix = FloatArray(16)
    private val mvpMatrix = FloatArray(16)
    private val tempMatrix = FloatArray(16)
    private val normalMatrix4 = FloatArray(16)
    private val normalMatrixTemp = FloatArray(16)
    private val normalMatrix3 = FloatArray(9)
    private val cameraPosition = floatArrayOf(0f, 0f, 5f)

    fun setFlavor(flavor: CitrosFlavor) {
        currentFlavor = flavor
        palette = HeroPalette.fromFlavor(flavor = currentFlavor, isDark = isDarkTheme)
    }

    fun setThemeIsDark(isDark: Boolean) {
        isDarkTheme = isDark
        palette = HeroPalette.fromFlavor(flavor = currentFlavor, isDark = isDarkTheme)
    }

    fun setParticleSizeScale(scale: Float) {
        particleSizeScale = scale.coerceIn(0.2f, 2.5f)
    }

    fun setClipCircle(enabled: Boolean) {
        clipCircle = enabled
    }

    override fun onSurfaceCreated(unused: javax.microedition.khronos.opengles.GL10?, config: javax.microedition.khronos.egl.EGLConfig?) {
        GLES20.glClearColor(0f, 0f, 0f, 0f)
        GLES20.glEnable(GLES20.GL_BLEND)
        GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA)
        GLES20.glEnable(GLES20.GL_DEPTH_TEST)
        GLES20.glDepthFunc(GLES20.GL_LEQUAL)
        GLES20.glDisable(GLES20.GL_CULL_FACE)

        sphereProgram = createSphereProgram()
        lineProgram = createLineProgram()
        pointProgram = createPointProgram()

        sphereMesh = buildIcosphere(subdivisions = 3)
        wireframeEdges = buildEdgeMesh(requireNotNull(sphereMesh).indexArray)
        particleField = buildParticleField(count = 1100)
        ringMesh = buildRingMesh(segments = 160, radius = 2.0f)

        startNanos = SystemClock.elapsedRealtimeNanos()
    }

    override fun onSurfaceChanged(unused: javax.microedition.khronos.opengles.GL10?, width: Int, height: Int) {
        viewportWidth = width.coerceAtLeast(1)
        viewportHeight = height.coerceAtLeast(1)
        GLES20.glViewport(0, 0, viewportWidth, viewportHeight)
        Matrix.perspectiveM(
            projectionMatrix,
            0,
            45f,
            viewportWidth.toFloat() / viewportHeight.toFloat(),
            0.1f,
            100f
        )
    }

    override fun onDrawFrame(unused: javax.microedition.khronos.opengles.GL10?) {
        val sphereProgram = sphereProgram ?: return
        val lineProgram = lineProgram ?: return
        val pointProgram = pointProgram ?: return
        val sphereMesh = sphereMesh ?: return
        val wireframeEdges = wireframeEdges ?: return
        val particleField = particleField ?: return
        val ringMesh = ringMesh ?: return

        val elapsedSeconds = ((SystemClock.elapsedRealtimeNanos() - startNanos) / 1_000_000_000.0f)
        val palette = palette
        val renderLightBackground = !isDarkTheme && !clipCircle
        if (renderLightBackground) {
            GLES20.glClearColor(1f, 1f, 1f, 1f)
        } else {
            GLES20.glClearColor(0f, 0f, 0f, 0f)
        }

        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT or GLES20.GL_DEPTH_BUFFER_BIT)

        Matrix.setLookAtM(
            viewMatrix,
            0,
            cameraPosition[0],
            cameraPosition[1],
            cameraPosition[2],
            0f,
            0f,
            0f,
            0f,
            1f,
            0f
        )

        // Sphere motion mirrors the Three.js animation constants.
        val rotationY = elapsedSeconds * 0.08f * RAD_TO_DEG
        val rotationX = sin(elapsedSeconds * 0.05f) * 0.15f * RAD_TO_DEG
        val scale = 1.2f * (1f + sin(elapsedSeconds * 0.4f) * 0.02f)

        Matrix.setIdentityM(modelMatrix, 0)
        Matrix.rotateM(modelMatrix, 0, rotationY, 0f, 1f, 0f)
        Matrix.rotateM(modelMatrix, 0, rotationX, 1f, 0f, 0f)
        Matrix.scaleM(modelMatrix, 0, scale, scale, scale)
        updateMvp(modelMatrix)
        updateNormalMatrix(modelMatrix)

        drawSphere(
            program = sphereProgram,
            mesh = sphereMesh,
            elapsedSeconds = elapsedSeconds,
            palette = palette
        )

        // Faint wireframe shell.
        Matrix.setIdentityM(modelMatrix, 0)
        Matrix.rotateM(modelMatrix, 0, rotationY, 0f, 1f, 0f)
        Matrix.rotateM(modelMatrix, 0, rotationX, 1f, 0f, 0f)
        Matrix.scaleM(modelMatrix, 0, scale * 1.017f, scale * 1.017f, scale * 1.017f)
        updateMvp(modelMatrix)
        drawWireframe(
            program = lineProgram,
            sphereMesh = sphereMesh,
            edgeMesh = wireframeEdges,
            color = palette.wireColor,
            alpha = 0.10f
        )

        // Orbital rings.
        drawRing(
            program = lineProgram,
            ringMesh = ringMesh,
            elapsedSeconds = elapsedSeconds,
            rotationX = 63f,
            rotationY = 18f,
            rotationZ = elapsedSeconds * 0.12f * RAD_TO_DEG,
            scale = 1.0f,
            alpha = 0.14f,
            color = palette.ringColor
        )
        drawRing(
            program = lineProgram,
            ringMesh = ringMesh,
            elapsedSeconds = elapsedSeconds,
            rotationX = 108f,
            rotationY = 90f,
            rotationZ = -elapsedSeconds * 0.08f * RAD_TO_DEG,
            scale = 1.0f,
            alpha = 0.14f,
            color = palette.ringColor
        )
        drawRing(
            program = lineProgram,
            ringMesh = ringMesh,
            elapsedSeconds = elapsedSeconds,
            rotationX = 27f,
            rotationY = 153f,
            rotationZ = elapsedSeconds * 0.05f * RAD_TO_DEG,
            scale = 1.3f,
            alpha = 0.14f,
            color = palette.ringColor
        )

        // Background particle field.
        Matrix.setIdentityM(modelMatrix, 0)
        Matrix.rotateM(modelMatrix, 0, elapsedSeconds * 0.03f * RAD_TO_DEG, 0f, 1f, 0f)
        Matrix.rotateM(modelMatrix, 0, sin(elapsedSeconds * 0.02f) * 0.1f * RAD_TO_DEG, 1f, 0f, 0f)
        updateMvp(modelMatrix)
        drawParticles(
            program = pointProgram,
            particles = particleField,
            color = palette.particleColor,
            opacity = 1.0f,
            sizeScale = particleSizeScale
        )
    }

    private fun drawSphere(
        program: SphereProgram,
        mesh: SphereMesh,
        elapsedSeconds: Float,
        palette: HeroPalette
    ) {
        GLES20.glUseProgram(program.programId)
        GLES20.glUniformMatrix4fv(program.uMvpMatrix, 1, false, mvpMatrix, 0)
        GLES20.glUniformMatrix4fv(program.uModelMatrix, 1, false, modelMatrix, 0)
        GLES20.glUniformMatrix3fv(program.uNormalMatrix, 1, false, normalMatrix3, 0)
        GLES20.glUniform1f(program.uTime, elapsedSeconds)
        GLES20.glUniform1f(program.uNoiseAmp, 0.25f)
        GLES20.glUniform1f(program.uNoiseFreq, 1.4f)
        GLES20.glUniform1f(program.uOpacity, 1.0f)
        GLES20.glUniform3fv(program.uColor1, 1, palette.color1, 0)
        GLES20.glUniform3fv(program.uColor2, 1, palette.color2, 0)
        GLES20.glUniform3fv(program.uColor3, 1, palette.color3, 0)
        GLES20.glUniform3fv(program.uCameraPos, 1, cameraPosition, 0)
        GLES20.glUniform2f(program.uViewport, viewportWidth.toFloat(), viewportHeight.toFloat())
        GLES20.glUniform1f(program.uClipCircle, if (clipCircle) 1f else 0f)

        mesh.positionBuffer.position(0)
        mesh.normalBuffer.position(0)
        mesh.indexBuffer.position(0)
        GLES20.glEnableVertexAttribArray(program.aPosition)
        GLES20.glEnableVertexAttribArray(program.aNormal)
        GLES20.glVertexAttribPointer(program.aPosition, 3, GLES20.GL_FLOAT, false, 0, mesh.positionBuffer)
        GLES20.glVertexAttribPointer(program.aNormal, 3, GLES20.GL_FLOAT, false, 0, mesh.normalBuffer)
        GLES20.glDrawElements(GLES20.GL_TRIANGLES, mesh.indexCount, GLES20.GL_UNSIGNED_SHORT, mesh.indexBuffer)
        GLES20.glDisableVertexAttribArray(program.aPosition)
        GLES20.glDisableVertexAttribArray(program.aNormal)
    }

    private fun drawWireframe(
        program: LineProgram,
        sphereMesh: SphereMesh,
        edgeMesh: EdgeMesh,
        color: FloatArray,
        alpha: Float
    ) {
        GLES20.glUseProgram(program.programId)
        GLES20.glUniformMatrix4fv(program.uMvpMatrix, 1, false, mvpMatrix, 0)
        GLES20.glUniform4f(program.uColor, color[0], color[1], color[2], alpha)
        GLES20.glUniform2f(program.uViewport, viewportWidth.toFloat(), viewportHeight.toFloat())
        GLES20.glUniform1f(program.uClipCircle, if (clipCircle) 1f else 0f)
        sphereMesh.positionBuffer.position(0)
        edgeMesh.indexBuffer.position(0)
        GLES20.glEnableVertexAttribArray(program.aPosition)
        GLES20.glVertexAttribPointer(program.aPosition, 3, GLES20.GL_FLOAT, false, 0, sphereMesh.positionBuffer)
        GLES20.glDrawElements(GLES20.GL_LINES, edgeMesh.indexCount, GLES20.GL_UNSIGNED_SHORT, edgeMesh.indexBuffer)
        GLES20.glDisableVertexAttribArray(program.aPosition)
    }

    private fun drawRing(
        program: LineProgram,
        ringMesh: RingMesh,
        elapsedSeconds: Float,
        rotationX: Float,
        rotationY: Float,
        rotationZ: Float,
        scale: Float,
        alpha: Float,
        color: FloatArray
    ) {
        Matrix.setIdentityM(modelMatrix, 0)
        Matrix.rotateM(modelMatrix, 0, rotationY, 0f, 1f, 0f)
        Matrix.rotateM(modelMatrix, 0, rotationX, 1f, 0f, 0f)
        Matrix.rotateM(modelMatrix, 0, rotationZ, 0f, 0f, 1f)
        val pulse = 1f + sin(elapsedSeconds * 0.3f) * 0.02f
        Matrix.scaleM(modelMatrix, 0, scale * pulse, scale * pulse, scale * pulse)
        updateMvp(modelMatrix)

        GLES20.glUseProgram(program.programId)
        GLES20.glUniformMatrix4fv(program.uMvpMatrix, 1, false, mvpMatrix, 0)
        GLES20.glUniform4f(program.uColor, color[0], color[1], color[2], alpha)
        GLES20.glUniform2f(program.uViewport, viewportWidth.toFloat(), viewportHeight.toFloat())
        GLES20.glUniform1f(program.uClipCircle, if (clipCircle) 1f else 0f)
        ringMesh.vertexBuffer.position(0)
        GLES20.glEnableVertexAttribArray(program.aPosition)
        GLES20.glVertexAttribPointer(program.aPosition, 3, GLES20.GL_FLOAT, false, 0, ringMesh.vertexBuffer)
        GLES20.glDrawArrays(GLES20.GL_LINE_LOOP, 0, ringMesh.vertexCount)
        GLES20.glDisableVertexAttribArray(program.aPosition)
    }

    private fun drawParticles(
        program: PointProgram,
        particles: ParticleField,
        color: FloatArray,
        opacity: Float,
        sizeScale: Float
    ) {
        GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE)
        GLES20.glUseProgram(program.programId)
        GLES20.glUniformMatrix4fv(program.uMvpMatrix, 1, false, mvpMatrix, 0)
        GLES20.glUniform3fv(program.uColor, 1, color, 0)
        GLES20.glUniform1f(program.uOpacity, opacity)
        GLES20.glUniform1f(program.uPointScale, sizeScale)
        GLES20.glUniform2f(program.uViewport, viewportWidth.toFloat(), viewportHeight.toFloat())
        GLES20.glUniform1f(program.uClipCircle, if (clipCircle) 1f else 0f)
        particles.positionBuffer.position(0)
        particles.sizeBuffer.position(0)
        GLES20.glEnableVertexAttribArray(program.aPosition)
        GLES20.glEnableVertexAttribArray(program.aSize)
        GLES20.glVertexAttribPointer(program.aPosition, 3, GLES20.GL_FLOAT, false, 0, particles.positionBuffer)
        GLES20.glVertexAttribPointer(program.aSize, 1, GLES20.GL_FLOAT, false, 0, particles.sizeBuffer)
        GLES20.glDrawArrays(GLES20.GL_POINTS, 0, particles.count)
        GLES20.glDisableVertexAttribArray(program.aPosition)
        GLES20.glDisableVertexAttribArray(program.aSize)
        GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA)
    }

    private fun updateMvp(model: FloatArray) {
        Matrix.multiplyMM(tempMatrix, 0, viewMatrix, 0, model, 0)
        Matrix.multiplyMM(mvpMatrix, 0, projectionMatrix, 0, tempMatrix, 0)
    }

    private fun updateNormalMatrix(model: FloatArray) {
        Matrix.invertM(normalMatrix4, 0, model, 0)
        Matrix.transposeM(normalMatrixTemp, 0, normalMatrix4, 0)
        normalMatrixTemp.copyInto(normalMatrix4)
        normalMatrix3[0] = normalMatrix4[0]
        normalMatrix3[1] = normalMatrix4[1]
        normalMatrix3[2] = normalMatrix4[2]
        normalMatrix3[3] = normalMatrix4[4]
        normalMatrix3[4] = normalMatrix4[5]
        normalMatrix3[5] = normalMatrix4[6]
        normalMatrix3[6] = normalMatrix4[8]
        normalMatrix3[7] = normalMatrix4[9]
        normalMatrix3[8] = normalMatrix4[10]
    }
}

private data class SphereProgram(
    val programId: Int,
    val aPosition: Int,
    val aNormal: Int,
    val uMvpMatrix: Int,
    val uModelMatrix: Int,
    val uNormalMatrix: Int,
    val uTime: Int,
    val uNoiseAmp: Int,
    val uNoiseFreq: Int,
    val uOpacity: Int,
    val uColor1: Int,
    val uColor2: Int,
    val uColor3: Int,
    val uCameraPos: Int,
    val uViewport: Int,
    val uClipCircle: Int
)

private data class LineProgram(
    val programId: Int,
    val aPosition: Int,
    val uMvpMatrix: Int,
    val uColor: Int,
    val uViewport: Int,
    val uClipCircle: Int
)

private data class PointProgram(
    val programId: Int,
    val aPosition: Int,
    val aSize: Int,
    val uMvpMatrix: Int,
    val uColor: Int,
    val uOpacity: Int,
    val uPointScale: Int,
    val uViewport: Int,
    val uClipCircle: Int
)

private data class SphereMesh(
    val positionBuffer: FloatBuffer,
    val normalBuffer: FloatBuffer,
    val indexBuffer: ShortBuffer,
    val indexCount: Int,
    val indexArray: ShortArray
)

private data class EdgeMesh(
    val indexBuffer: ShortBuffer,
    val indexCount: Int
)

private data class ParticleField(
    val positionBuffer: FloatBuffer,
    val sizeBuffer: FloatBuffer,
    val count: Int
)

private data class RingMesh(
    val vertexBuffer: FloatBuffer,
    val vertexCount: Int
)

private data class HeroPalette(
    val color1: FloatArray,
    val color2: FloatArray,
    val color3: FloatArray,
    val wireColor: FloatArray,
    val ringColor: FloatArray,
    val particleColor: FloatArray
) {
    companion object {
        fun fromFlavor(flavor: CitrosFlavor, isDark: Boolean): HeroPalette {
            val deep = if (isDark) {
                lerp(Color(0xFF060607), flavor.tint, 0.78f)
            } else {
                lerp(Color(0xFFF8EFE5), flavor.glow, 0.30f)
            }
            val primary = if (isDark) {
                lerp(flavor.primary, flavor.glow, 0.08f)
            } else {
                lerp(flavor.primary, flavor.glow, 0.20f)
            }
            val warm = if (isDark) {
                lerp(flavor.primary, Color.White, 0.18f)
            } else {
                lerp(flavor.primary, Color(0xFFFCE9D2), 0.22f)
            }
            val wire = if (isDark) {
                lerp(flavor.tint, flavor.primary, 0.30f)
            } else {
                lerp(Color(0xFFC6A98D), flavor.primary, 0.24f)
            }
            val ring = if (isDark) {
                lerp(flavor.primary, flavor.glow, 0.34f)
            } else {
                lerp(flavor.primary, flavor.glow, 0.46f)
            }
            val particle = if (isDark) {
                lerp(flavor.primary, flavor.glow, 0.56f)
            } else {
                lerp(flavor.primary, flavor.glow, 0.62f)
            }
            return HeroPalette(
                color1 = deep.toRgbArray(),
                color2 = primary.toRgbArray(),
                color3 = warm.toRgbArray(),
                wireColor = wire.toRgbArray(),
                ringColor = ring.toRgbArray(),
                particleColor = particle.toRgbArray()
            )
        }
    }
}

private fun Color.toRgbArray(): FloatArray = floatArrayOf(red, green, blue)

private data class Vec3(val x: Float, val y: Float, val z: Float)

private fun buildIcosphere(subdivisions: Int): SphereMesh {
    val t = ((1.0 + sqrt(5.0)) / 2.0).toFloat()
    val vertices = mutableListOf(
        normalize(Vec3(-1f, t, 0f)),
        normalize(Vec3(1f, t, 0f)),
        normalize(Vec3(-1f, -t, 0f)),
        normalize(Vec3(1f, -t, 0f)),
        normalize(Vec3(0f, -1f, t)),
        normalize(Vec3(0f, 1f, t)),
        normalize(Vec3(0f, -1f, -t)),
        normalize(Vec3(0f, 1f, -t)),
        normalize(Vec3(t, 0f, -1f)),
        normalize(Vec3(t, 0f, 1f)),
        normalize(Vec3(-t, 0f, -1f)),
        normalize(Vec3(-t, 0f, 1f))
    )

    var faces = mutableListOf(
        intArrayOf(0, 11, 5), intArrayOf(0, 5, 1), intArrayOf(0, 1, 7), intArrayOf(0, 7, 10), intArrayOf(0, 10, 11),
        intArrayOf(1, 5, 9), intArrayOf(5, 11, 4), intArrayOf(11, 10, 2), intArrayOf(10, 7, 6), intArrayOf(7, 1, 8),
        intArrayOf(3, 9, 4), intArrayOf(3, 4, 2), intArrayOf(3, 2, 6), intArrayOf(3, 6, 8), intArrayOf(3, 8, 9),
        intArrayOf(4, 9, 5), intArrayOf(2, 4, 11), intArrayOf(6, 2, 10), intArrayOf(8, 6, 7), intArrayOf(9, 8, 1)
    )

    repeat(subdivisions.coerceAtLeast(0)) {
        val midpointCache = HashMap<Long, Int>()
        fun midpoint(i1: Int, i2: Int): Int {
            val min = minOf(i1, i2)
            val max = maxOf(i1, i2)
            val key = (min.toLong() shl 32) or max.toLong()
            val cached = midpointCache[key]
            if (cached != null) return cached
            val v1 = vertices[i1]
            val v2 = vertices[i2]
            val mid = normalize(Vec3((v1.x + v2.x) * 0.5f, (v1.y + v2.y) * 0.5f, (v1.z + v2.z) * 0.5f))
            vertices.add(mid)
            val index = vertices.lastIndex
            midpointCache[key] = index
            return index
        }

        val refined = ArrayList<IntArray>(faces.size * 4)
        faces.forEach { tri ->
            val a = midpoint(tri[0], tri[1])
            val b = midpoint(tri[1], tri[2])
            val c = midpoint(tri[2], tri[0])
            refined.add(intArrayOf(tri[0], a, c))
            refined.add(intArrayOf(tri[1], b, a))
            refined.add(intArrayOf(tri[2], c, b))
            refined.add(intArrayOf(a, b, c))
        }
        faces = refined
    }

    val positions = FloatArray(vertices.size * 3)
    vertices.forEachIndexed { index, v ->
        val cursor = index * 3
        positions[cursor] = v.x
        positions[cursor + 1] = v.y
        positions[cursor + 2] = v.z
    }

    val indexArray = ShortArray(faces.size * 3)
    var indexCursor = 0
    faces.forEach { tri ->
        indexArray[indexCursor++] = tri[0].toShort()
        indexArray[indexCursor++] = tri[1].toShort()
        indexArray[indexCursor++] = tri[2].toShort()
    }

    return SphereMesh(
        positionBuffer = positions.toFloatBuffer(),
        normalBuffer = positions.copyOf().toFloatBuffer(),
        indexBuffer = indexArray.toShortBuffer(),
        indexCount = indexArray.size,
        indexArray = indexArray
    )
}

private fun buildEdgeMesh(triangleIndices: ShortArray): EdgeMesh {
    val edges = ArrayList<Short>(triangleIndices.size * 2)
    val seen = HashSet<Long>()

    fun addEdge(a: Short, b: Short) {
        val ai = a.toInt() and 0xFFFF
        val bi = b.toInt() and 0xFFFF
        val min = minOf(ai, bi)
        val max = maxOf(ai, bi)
        val key = (min.toLong() shl 32) or max.toLong()
        if (seen.add(key)) {
            edges.add(a)
            edges.add(b)
        }
    }

    var cursor = 0
    while (cursor + 2 < triangleIndices.size) {
        val i0 = triangleIndices[cursor]
        val i1 = triangleIndices[cursor + 1]
        val i2 = triangleIndices[cursor + 2]
        addEdge(i0, i1)
        addEdge(i1, i2)
        addEdge(i2, i0)
        cursor += 3
    }

    val edgeArray = ShortArray(edges.size)
    edges.forEachIndexed { index, value -> edgeArray[index] = value }
    return EdgeMesh(
        indexBuffer = edgeArray.toShortBuffer(),
        indexCount = edgeArray.size
    )
}

private fun buildParticleField(count: Int): ParticleField {
    val random = Random(20260217)
    val positions = FloatArray(count * 3)
    val sizes = FloatArray(count)
    var cursor = 0
    for (i in 0 until count) {
        val theta = random.nextFloat() * TWO_PI
        val phi = acos((2f * random.nextFloat()) - 1f)
        val radius = 2.0f + random.nextFloat() * 3.5f
        positions[cursor++] = radius * sin(phi) * cos(theta)
        positions[cursor++] = radius * sin(phi) * sin(theta)
        positions[cursor++] = radius * cos(phi)
        sizes[i] = 0.5f + random.nextFloat() * 2.0f
    }
    return ParticleField(
        positionBuffer = positions.toFloatBuffer(),
        sizeBuffer = sizes.toFloatBuffer(),
        count = count
    )
}

private fun buildRingMesh(segments: Int, radius: Float): RingMesh {
    val safeSegments = segments.coerceAtLeast(3)
    val vertices = FloatArray(safeSegments * 3)
    var cursor = 0
    for (i in 0 until safeSegments) {
        val angle = (i.toFloat() / safeSegments.toFloat()) * TWO_PI
        vertices[cursor++] = cos(angle) * radius
        vertices[cursor++] = sin(angle) * radius
        vertices[cursor++] = 0f
    }
    return RingMesh(
        vertexBuffer = vertices.toFloatBuffer(),
        vertexCount = safeSegments
    )
}

private fun createSphereProgram(): SphereProgram {
    val programId = createProgram(SPHERE_VERTEX_SHADER, SPHERE_FRAGMENT_SHADER)
    return SphereProgram(
        programId = programId,
        aPosition = GLES20.glGetAttribLocation(programId, "aPosition"),
        aNormal = GLES20.glGetAttribLocation(programId, "aNormal"),
        uMvpMatrix = GLES20.glGetUniformLocation(programId, "uMvpMatrix"),
        uModelMatrix = GLES20.glGetUniformLocation(programId, "uModelMatrix"),
        uNormalMatrix = GLES20.glGetUniformLocation(programId, "uNormalMatrix"),
        uTime = GLES20.glGetUniformLocation(programId, "uTime"),
        uNoiseAmp = GLES20.glGetUniformLocation(programId, "uNoiseAmp"),
        uNoiseFreq = GLES20.glGetUniformLocation(programId, "uNoiseFreq"),
        uOpacity = GLES20.glGetUniformLocation(programId, "uOpacity"),
        uColor1 = GLES20.glGetUniformLocation(programId, "uColor1"),
        uColor2 = GLES20.glGetUniformLocation(programId, "uColor2"),
        uColor3 = GLES20.glGetUniformLocation(programId, "uColor3"),
        uCameraPos = GLES20.glGetUniformLocation(programId, "uCameraPos"),
        uViewport = GLES20.glGetUniformLocation(programId, "uViewport"),
        uClipCircle = GLES20.glGetUniformLocation(programId, "uClipCircle")
    )
}

private fun createLineProgram(): LineProgram {
    val programId = createProgram(LINE_VERTEX_SHADER, LINE_FRAGMENT_SHADER)
    return LineProgram(
        programId = programId,
        aPosition = GLES20.glGetAttribLocation(programId, "aPosition"),
        uMvpMatrix = GLES20.glGetUniformLocation(programId, "uMvpMatrix"),
        uColor = GLES20.glGetUniformLocation(programId, "uColor"),
        uViewport = GLES20.glGetUniformLocation(programId, "uViewport"),
        uClipCircle = GLES20.glGetUniformLocation(programId, "uClipCircle")
    )
}

private fun createPointProgram(): PointProgram {
    val programId = createProgram(POINT_VERTEX_SHADER, POINT_FRAGMENT_SHADER)
    return PointProgram(
        programId = programId,
        aPosition = GLES20.glGetAttribLocation(programId, "aPosition"),
        aSize = GLES20.glGetAttribLocation(programId, "aSize"),
        uMvpMatrix = GLES20.glGetUniformLocation(programId, "uMvpMatrix"),
        uColor = GLES20.glGetUniformLocation(programId, "uColor"),
        uOpacity = GLES20.glGetUniformLocation(programId, "uOpacity"),
        uPointScale = GLES20.glGetUniformLocation(programId, "uPointScale"),
        uViewport = GLES20.glGetUniformLocation(programId, "uViewport"),
        uClipCircle = GLES20.glGetUniformLocation(programId, "uClipCircle")
    )
}

private fun normalize(v: Vec3): Vec3 {
    val length = sqrt((v.x * v.x + v.y * v.y + v.z * v.z).toDouble()).toFloat().coerceAtLeast(1e-6f)
    return Vec3(v.x / length, v.y / length, v.z / length)
}

private fun FloatArray.toFloatBuffer(): FloatBuffer {
    val buffer = ByteBuffer.allocateDirect(size * 4)
        .order(ByteOrder.nativeOrder())
        .asFloatBuffer()
    buffer.put(this)
    buffer.position(0)
    return buffer
}

private fun ShortArray.toShortBuffer(): ShortBuffer {
    val buffer = ByteBuffer.allocateDirect(size * 2)
        .order(ByteOrder.nativeOrder())
        .asShortBuffer()
    buffer.put(this)
    buffer.position(0)
    return buffer
}

private fun createProgram(vertexShaderCode: String, fragmentShaderCode: String): Int {
    val vertexShader = compileShader(GLES20.GL_VERTEX_SHADER, vertexShaderCode)
    val fragmentShader = compileShader(GLES20.GL_FRAGMENT_SHADER, fragmentShaderCode)
    val program = GLES20.glCreateProgram()
    if (program == 0) {
        GLES20.glDeleteShader(vertexShader)
        GLES20.glDeleteShader(fragmentShader)
        error("Failed to create GL program")
    }
    GLES20.glAttachShader(program, vertexShader)
    GLES20.glAttachShader(program, fragmentShader)
    GLES20.glLinkProgram(program)
    val linkStatus = IntArray(1)
    GLES20.glGetProgramiv(program, GLES20.GL_LINK_STATUS, linkStatus, 0)
    if (linkStatus[0] == 0) {
        val log = GLES20.glGetProgramInfoLog(program)
        GLES20.glDeleteProgram(program)
        GLES20.glDeleteShader(vertexShader)
        GLES20.glDeleteShader(fragmentShader)
        error("GL program link failed: $log")
    }
    GLES20.glDeleteShader(vertexShader)
    GLES20.glDeleteShader(fragmentShader)
    return program
}

private fun compileShader(type: Int, shaderCode: String): Int {
    val shader = GLES20.glCreateShader(type)
    if (shader == 0) {
        error("Failed to create shader type=$type")
    }
    GLES20.glShaderSource(shader, shaderCode)
    GLES20.glCompileShader(shader)
    val compileStatus = IntArray(1)
    GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, compileStatus, 0)
    if (compileStatus[0] == 0) {
        val log = GLES20.glGetShaderInfoLog(shader)
        GLES20.glDeleteShader(shader)
        error("GL shader compile failed: $log")
    }
    return shader
}

private const val TWO_PI = (Math.PI * 2.0f).toFloat()
private const val RAD_TO_DEG = (180.0f / Math.PI).toFloat()

private const val SPHERE_VERTEX_SHADER = """
    precision highp float;
    attribute vec3 aPosition;
    attribute vec3 aNormal;

    uniform mat4 uMvpMatrix;
    uniform mat4 uModelMatrix;
    uniform mat3 uNormalMatrix;
    uniform float uTime;
    uniform float uNoiseAmp;
    uniform float uNoiseFreq;

    varying float vDisplacement;
    varying vec3 vNormal;
    varying vec3 vWorldPos;

    vec3 mod289(vec3 x){ return x - floor(x*(1.0/289.0))*289.0; }
    vec4 mod289(vec4 x){ return x - floor(x*(1.0/289.0))*289.0; }
    vec4 permute(vec4 x){ return mod289(((x*34.0)+1.0)*x); }
    vec4 taylorInvSqrt(vec4 r){ return 1.79284291400159 - 0.85373472095314*r; }

    float snoise(vec3 v){
      const vec2 C = vec2(1.0/6.0, 1.0/3.0);
      const vec4 D = vec4(0.0, 0.5, 1.0, 2.0);
      vec3 i = floor(v + dot(v, C.yyy));
      vec3 x0 = v - i + dot(i, C.xxx);
      vec3 g = step(x0.yzx, x0.xyz);
      vec3 l = 1.0 - g;
      vec3 i1 = min(g.xyz, l.zxy);
      vec3 i2 = max(g.xyz, l.zxy);
      vec3 x1 = x0 - i1 + C.xxx;
      vec3 x2 = x0 - i2 + C.yyy;
      vec3 x3 = x0 - D.yyy;
      i = mod289(i);
      vec4 p = permute(permute(permute(
        i.z + vec4(0.0, i1.z, i2.z, 1.0))
        + i.y + vec4(0.0, i1.y, i2.y, 1.0))
        + i.x + vec4(0.0, i1.x, i2.x, 1.0));
      float n_ = 0.142857142857;
      vec3 ns = n_ * D.wyz - D.xzx;
      vec4 j = p - 49.0*floor(p*ns.z*ns.z);
      vec4 x_ = floor(j*ns.z);
      vec4 y_ = floor(j - 7.0*x_);
      vec4 x = x_ * ns.x + ns.yyyy;
      vec4 y = y_ * ns.x + ns.yyyy;
      vec4 h = 1.0 - abs(x) - abs(y);
      vec4 b0 = vec4(x.xy, y.xy);
      vec4 b1 = vec4(x.zw, y.zw);
      vec4 s0 = floor(b0)*2.0 + 1.0;
      vec4 s1 = floor(b1)*2.0 + 1.0;
      vec4 sh = -step(h, vec4(0.0));
      vec4 a0 = b0.xzyw + s0.xzyw*sh.xxyy;
      vec4 a1 = b1.xzyw + s1.xzyw*sh.zzww;
      vec3 p0 = vec3(a0.xy, h.x);
      vec3 p1 = vec3(a0.zw, h.y);
      vec3 p2 = vec3(a1.xy, h.z);
      vec3 p3 = vec3(a1.zw, h.w);
      vec4 norm = taylorInvSqrt(vec4(dot(p0,p0),dot(p1,p1),dot(p2,p2),dot(p3,p3)));
      p0 *= norm.x; p1 *= norm.y; p2 *= norm.z; p3 *= norm.w;
      vec4 m = max(0.6 - vec4(dot(x0,x0),dot(x1,x1),dot(x2,x2),dot(x3,x3)), 0.0);
      m = m * m;
      return 42.0 * dot(m*m, vec4(dot(p0,x0),dot(p1,x1),dot(p2,x2),dot(p3,x3)));
    }

    void main() {
      float t = uTime * 0.25;
      float n1 = snoise(aPosition * uNoiseFreq + t);
      float n2 = snoise(aPosition * uNoiseFreq * 2.0 + t * 1.3) * 0.5;
      float noise = n1 + n2;
      float displacement = noise * uNoiseAmp;
      vec3 newPos = aPosition + aNormal * displacement;

      vDisplacement = noise;
      vNormal = normalize(uNormalMatrix * aNormal);
      vWorldPos = (uModelMatrix * vec4(newPos, 1.0)).xyz;
      gl_Position = uMvpMatrix * vec4(newPos, 1.0);
    }
"""

private const val SPHERE_FRAGMENT_SHADER = """
    precision highp float;

    uniform float uOpacity;
    uniform vec3 uColor1;
    uniform vec3 uColor2;
    uniform vec3 uColor3;
    uniform vec3 uCameraPos;
    uniform vec2 uViewport;
    uniform float uClipCircle;

    varying float vDisplacement;
    varying vec3 vNormal;
    varying vec3 vWorldPos;

    void applyCircularClip() {
      if (uClipCircle < 0.5) return;
      float minSide = min(uViewport.x, uViewport.y);
      vec2 centered = (gl_FragCoord.xy - 0.5 * uViewport) / minSide;
      if (length(centered) > 0.5) discard;
    }

    void main() {
      applyCircularClip();
      vec3 viewDir = normalize(uCameraPos - vWorldPos);
      float fresnel = pow(1.0 - max(dot(vNormal, viewDir), 0.0), 3.0);
      float t = vDisplacement * 0.5 + 0.5;
      vec3 color = mix(uColor1, uColor2, t);
      color = mix(color, uColor3, fresnel * 0.6);
      color += fresnel * uColor2 * 0.4;
      float alpha = (0.88 + fresnel * 0.12) * uOpacity;
      gl_FragColor = vec4(color, alpha);
    }
"""

private const val LINE_VERTEX_SHADER = """
    precision mediump float;
    attribute vec3 aPosition;
    uniform mat4 uMvpMatrix;
    void main() {
      gl_Position = uMvpMatrix * vec4(aPosition, 1.0);
    }
"""

private const val LINE_FRAGMENT_SHADER = """
    precision mediump float;
    uniform vec4 uColor;
    uniform vec2 uViewport;
    uniform float uClipCircle;

    void applyCircularClip() {
      if (uClipCircle < 0.5) return;
      float minSide = min(uViewport.x, uViewport.y);
      vec2 centered = (gl_FragCoord.xy - 0.5 * uViewport) / minSide;
      if (length(centered) > 0.5) discard;
    }

    void main() {
      applyCircularClip();
      gl_FragColor = uColor;
    }
"""

private const val POINT_VERTEX_SHADER = """
    precision mediump float;
    attribute vec3 aPosition;
    attribute float aSize;
    uniform mat4 uMvpMatrix;
    uniform float uPointScale;
    varying float vAlpha;
    void main() {
      float dist = length(aPosition);
      vAlpha = smoothstep(5.5, 2.0, dist) * 0.6;
      gl_Position = uMvpMatrix * vec4(aPosition, 1.0);
      gl_PointSize = aSize * uPointScale * (200.0 / max(gl_Position.w, 0.001));
    }
"""

private const val POINT_FRAGMENT_SHADER = """
    precision mediump float;
    uniform vec3 uColor;
    uniform float uOpacity;
    uniform vec2 uViewport;
    uniform float uClipCircle;
    varying float vAlpha;

    void applyCircularClip() {
      if (uClipCircle < 0.5) return;
      float minSide = min(uViewport.x, uViewport.y);
      vec2 centered = (gl_FragCoord.xy - 0.5 * uViewport) / minSide;
      if (length(centered) > 0.5) discard;
    }

    void main() {
      applyCircularClip();
      float d = length(gl_PointCoord - vec2(0.5)) * 2.0;
      float circle = 1.0 - smoothstep(0.4, 1.0, d);
      gl_FragColor = vec4(uColor, circle * vAlpha * uOpacity);
    }
"""
