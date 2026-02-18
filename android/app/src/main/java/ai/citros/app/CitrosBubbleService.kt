package ai.citros.app

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.PixelFormat
import android.os.Build
import android.os.IBinder
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.WebSettings
import android.webkit.WebView
import android.widget.FrameLayout

/**
 * Draggable FrameLayout that intercepts touch events for drag handling
 * while still allowing the WebView to receive non-drag touches.
 */
class DraggableFrameLayout(context: Context) : FrameLayout(context) {
    var onDrag: ((dx: Int, dy: Int) -> Unit)? = null
    var onTap: (() -> Unit)? = null

    private var initialTouchX = 0f
    private var initialTouchY = 0f
    private var isDragging = false
    private val dragThreshold = 20f

    override fun onInterceptTouchEvent(ev: MotionEvent): Boolean {
        when (ev.action) {
            MotionEvent.ACTION_DOWN -> {
                initialTouchX = ev.rawX
                initialTouchY = ev.rawY
                isDragging = false
                return false // Let children get the DOWN event
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = ev.rawX - initialTouchX
                val dy = ev.rawY - initialTouchY
                if (dx * dx + dy * dy > dragThreshold * dragThreshold) {
                    isDragging = true
                    return true // Intercept — start dragging
                }
            }
        }
        return false
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.action) {
            MotionEvent.ACTION_MOVE -> {
                if (isDragging) {
                    val dx = (event.rawX - initialTouchX).toInt()
                    val dy = (event.rawY - initialTouchY).toInt()
                    onDrag?.invoke(dx, dy)
                }
                return true
            }
            MotionEvent.ACTION_UP -> {
                if (!isDragging) {
                    onTap?.invoke()
                }
                // Reset for next gesture: update initial positions
                isDragging = false
                return true
            }
        }
        return true
    }
}

class CitrosBubbleService : Service() {

    companion object {
        const val CHANNEL_ID = "citros_bubble_channel"
        const val NOTIFICATION_ID = 1001
        const val ACTION_STOP = "ai.citros.app.ACTION_STOP_BUBBLE"
        private const val BUBBLE_SIZE_DP = 120
    }

    private lateinit var windowManager: WindowManager
    private var bubbleView: DraggableFrameLayout? = null
    private var webView: WebView? = null
    private lateinit var params: WindowManager.LayoutParams

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())
        createBubbleOverlay()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onDestroy() {
        super.onDestroy()
        webView?.destroy()
        bubbleView?.let {
            try {
                windowManager.removeView(it)
            } catch (_: Exception) {}
        }
        bubbleView = null
        webView = null
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Citros Bubble",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = "Keeps the Citros bubble overlay running"
            setShowBadge(false)
        }
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification {
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val openPending = PendingIntent.getActivity(
            this, 0, openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val stopIntent = Intent(this, CitrosBubbleService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPending = PendingIntent.getService(
            this, 1, stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Citros is running")
            .setContentText("Tap to open, or swipe to dismiss")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(openPending)
            .addAction(
                Notification.Action.Builder(
                    null, "Stop", stopPending
                ).build()
            )
            .setOngoing(true)
            .build()
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun createBubbleOverlay() {
        windowManager = getSystemService(Context.WINDOW_SERVICE) as WindowManager

        val bubbleSizePx = (BUBBLE_SIZE_DP * resources.displayMetrics.density).toInt()

        params = WindowManager.LayoutParams(
            bubbleSizePx,
            bubbleSizePx,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                    WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS or
                    WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED,
            PixelFormat.TRANSLUCENT
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            val dm = resources.displayMetrics
            x = dm.widthPixels - bubbleSizePx - (16 * dm.density).toInt()
            y = dm.heightPixels - bubbleSizePx - (100 * dm.density).toInt()
        }

        val container = DraggableFrameLayout(this)
        container.setBackgroundColor(Color.TRANSPARENT)

        // Track drag start position for relative movement
        var dragStartX = 0
        var dragStartY = 0

        container.onDrag = { dx, dy ->
            params.x = dragStartX + dx
            params.y = dragStartY + dy
            try {
                windowManager.updateViewLayout(container, params)
            } catch (_: Exception) {}
        }

        // Save position at touch start
        container.setOnTouchListener { _, event ->
            if (event.action == MotionEvent.ACTION_DOWN) {
                dragStartX = params.x
                dragStartY = params.y
            }
            false // Don't consume — let DraggableFrameLayout handle
        }

        container.onTap = {
            // Cycle through states on tap
            webView?.evaluateJavascript("""
                (function() {
                    var states = ['idle','listening','thinking','speaking','attention','error'];
                    var current = states.indexOf(currentStateName);
                    var next = (current + 1) % states.length;
                    setState(states[next]);
                })();
            """.trimIndent(), null)
        }

        val wv = WebView(this).apply {
            setBackgroundColor(Color.TRANSPARENT)
            setLayerType(View.LAYER_TYPE_HARDWARE, null)

            settings.apply {
                javaScriptEnabled = true
                domStorageEnabled = true
                allowFileAccess = true
                mediaPlaybackRequiresUserGesture = false
                cacheMode = WebSettings.LOAD_DEFAULT
                useWideViewPort = false
                loadWithOverviewMode = false
            }

            // Force dark mode off so colors render correctly
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                isForceDarkAllowed = false
            }

            loadUrl("file:///android_asset/bubble.html")

            // Force WebView to keep animating even when not focused
            onResume()
            resumeTimers()
        }

        webView = wv

        container.addView(wv, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT,
            FrameLayout.LayoutParams.MATCH_PARENT
        ))

        bubbleView = container
        windowManager.addView(container, params)
    }
}
