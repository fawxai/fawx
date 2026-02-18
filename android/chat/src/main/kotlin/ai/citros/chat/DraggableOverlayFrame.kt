package ai.citros.chat

import android.content.Context
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.WindowManager
import android.widget.FrameLayout

/**
 * A FrameLayout wrapper that intercepts drag gestures before they reach
 * the child ComposeView. Solves the fundamental issue where ComposeView
 * (a ViewGroup) dispatches touch events to its Compose children first,
 * preventing View-level onTouchListener from ever receiving them.
 *
 * Uses Android's standard onInterceptTouchEvent pattern:
 * 1. ACTION_DOWN: Let child (Compose) see it for clicks/long-press
 * 2. ACTION_MOVE: Once drag threshold is exceeded, intercept the gesture
 * 3. After interception: Handle all subsequent events for window dragging
 */
class DraggableOverlayFrame(context: Context) : FrameLayout(context) {

    /** Callback for drag events — implemented by OverlayService. */
    interface Callback {
        /** Called on each drag move with updated x/y (already written to params). */
        fun onDragMove(x: Int, y: Int)
        /** Called on drag end with fling velocity and release coordinates. */
        fun onDragEnd(velocityY: Float, rawX: Float, rawY: Float)
    }

    var callback: Callback? = null
    var overlayParams: WindowManager.LayoutParams? = null
    /** When false, all touch events pass through to Compose children (mini-chat scroll). */
    var dragEnabled: Boolean = true

    private var dragStartX = 0
    private var dragStartY = 0
    private var initialTouchX = 0f
    private var initialTouchY = 0f
    private var isDragging = false
    private var velocityTracker: VelocityTracker? = null

    private val dragThresholdPx: Float
        get() = DRAG_THRESHOLD_DP * resources.displayMetrics.density

    override fun onInterceptTouchEvent(ev: MotionEvent): Boolean {
        if (!dragEnabled) return false
        when (ev.action) {
            MotionEvent.ACTION_DOWN -> {
                val p = overlayParams ?: return false
                initialTouchX = ev.rawX
                initialTouchY = ev.rawY
                dragStartX = p.x
                dragStartY = p.y
                isDragging = false
                velocityTracker?.recycle()
                velocityTracker = VelocityTracker.obtain()
                velocityTracker?.addMovement(ev)
                return false // Let Compose see DOWN for clicks/long-press
            }
            MotionEvent.ACTION_MOVE -> {
                velocityTracker?.addMovement(ev)
                val dx = ev.rawX - initialTouchX
                val dy = ev.rawY - initialTouchY
                if (dx * dx + dy * dy > dragThresholdPx * dragThresholdPx) {
                    isDragging = true
                    return true // Steal gesture from Compose
                }
                return false
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                // Always clean up tracker if not dragging. If dragging,
                // onTouchEvent handles cleanup on ACTION_UP/CANCEL.
                if (!isDragging) {
                    velocityTracker?.recycle()
                    velocityTracker = null
                } else if (ev.action == MotionEvent.ACTION_CANCEL) {
                    // Edge case: CANCEL while isDragging but before interception completes
                    velocityTracker?.recycle()
                    velocityTracker = null
                    isDragging = false
                }
                return false
            }
        }
        return false
    }

    override fun onTouchEvent(ev: MotionEvent): Boolean {
        val p = overlayParams ?: return false
        when (ev.action) {
            MotionEvent.ACTION_MOVE -> {
                velocityTracker?.addMovement(ev)
                p.x = dragStartX + (ev.rawX - initialTouchX).toInt()
                p.y = dragStartY + (ev.rawY - initialTouchY).toInt()
                callback?.onDragMove(p.x, p.y)
                return true
            }
            MotionEvent.ACTION_UP -> {
                velocityTracker?.apply {
                    addMovement(ev)
                    computeCurrentVelocity(1000)
                    callback?.onDragEnd(yVelocity, ev.rawX, ev.rawY)
                }
                velocityTracker?.recycle()
                velocityTracker = null
                isDragging = false
                return true
            }
            MotionEvent.ACTION_CANCEL -> {
                velocityTracker?.recycle()
                velocityTracker = null
                isDragging = false
                return true
            }
        }
        return false
    }

    companion object {
        private const val DRAG_THRESHOLD_DP = 8f
    }
}
