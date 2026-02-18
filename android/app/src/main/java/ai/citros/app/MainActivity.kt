package ai.citros.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class MainActivity : ComponentActivity() {

    private val bubbleRunning = mutableStateOf(false)
    private val overlayGranted = mutableStateOf(false)

    private val overlayPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) {
        overlayGranted.value = Settings.canDrawOverlays(this)
        if (overlayGranted.value) {
            startBubble()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        overlayGranted.value = Settings.canDrawOverlays(this)

        setContent {
            MaterialTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center
                    ) {
                        Text(
                            text = "🍊",
                            fontSize = 64.sp
                        )
                        Spacer(modifier = Modifier.height(16.dp))
                        Text(
                            text = "Citros",
                            fontSize = 32.sp,
                            fontWeight = FontWeight.Bold
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = "Tangerine",
                            fontSize = 16.sp,
                            color = Color(0xFFFF8C00)
                        )
                        Spacer(modifier = Modifier.height(32.dp))

                        if (!overlayGranted.value) {
                            Text(
                                text = "Overlay permission required",
                                fontSize = 14.sp,
                                color = Color.Gray
                            )
                            Spacer(modifier = Modifier.height(12.dp))
                            Button(onClick = { requestOverlayPermission() }) {
                                Text("Grant Permission")
                            }
                        } else if (!bubbleRunning.value) {
                            Button(onClick = { startBubble() }) {
                                Text("Start Bubble")
                            }
                        } else {
                            Text(
                                text = "Citros is running ✓",
                                fontSize = 16.sp,
                                color = Color(0xFF4CAF50)
                            )
                            Spacer(modifier = Modifier.height(12.dp))
                            Button(
                                onClick = { stopBubble() },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = Color(0xFFE53935)
                                )
                            ) {
                                Text("Stop Bubble", color = Color.White)
                            }
                        }
                    }
                }
            }
        }

        // Auto-start if permission already granted
        if (overlayGranted.value) {
            startBubble()
        }
    }

    override fun onResume() {
        super.onResume()
        overlayGranted.value = Settings.canDrawOverlays(this)
    }

    private fun requestOverlayPermission() {
        val intent = Intent(
            Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
            Uri.parse("package:$packageName")
        )
        overlayPermissionLauncher.launch(intent)
    }

    private fun startBubble() {
        val intent = Intent(this, CitrosBubbleService::class.java)
        startForegroundService(intent)
        bubbleRunning.value = true
    }

    private fun stopBubble() {
        val intent = Intent(this, CitrosBubbleService::class.java).apply {
            action = CitrosBubbleService.ACTION_STOP
        }
        startService(intent)
        bubbleRunning.value = false
    }
}
