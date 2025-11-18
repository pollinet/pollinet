package xyz.pollinet.android.ui

import android.util.Base64
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import xyz.pollinet.sdk.BroadcastPreparation
import xyz.pollinet.sdk.PolliNetSDK

/**
 * UI component for visualizing BLE mesh broadcasting
 * 
 * Features:
 * - Real-time broadcast status
 * - Fragment transmission progress
 * - Animated visualization
 * - Transaction details
 */
@Composable
fun BroadcastVisualizationCard(
    sdk: PolliNetSDK?,
    modifier: Modifier = Modifier
) {
    var broadcastState by remember { mutableStateOf<BroadcastState>(BroadcastState.Idle) }
    var currentBroadcast by remember { mutableStateOf<BroadcastPreparation?>(null) }
    var sentFragments by remember { mutableStateOf(0) }
    var testTransactionSize by remember { mutableStateOf(350) }
    val scope = rememberCoroutineScope()

    Card(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp)
            .wrapContentHeight(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f))
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Text(
                text = "📡 BLE Mesh Broadcaster",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold
            )

            // Status indicator
            BroadcastStatusIndicator(broadcastState)

            // Transaction size selector
            TransactionSizeSelector(
                currentSize = testTransactionSize,
                onSizeChanged = { testTransactionSize = it },
                enabled = broadcastState == BroadcastState.Idle
            )

            // Broadcast info
            currentBroadcast?.let { broadcast ->
                BroadcastInfo(
                    broadcast = broadcast,
                    sentFragments = sentFragments,
                    state = broadcastState
                )
            }

            // Control buttons
            BroadcastControls(
                state = broadcastState,
                onPrepare = {
                    if (sdk == null) {
                        broadcastState = BroadcastState.Error("SDK not initialized")
                        return@BroadcastControls
                    }

                    broadcastState = BroadcastState.Preparing
                    scope.launch(Dispatchers.IO) {
                        try {
                            // Create test transaction
                            val txBytes = ByteArray(testTransactionSize) { it.toByte() }

                            // Prepare broadcast
                            val result = sdk.prepareBroadcast(txBytes)

                            result.onSuccess { prep ->
                                currentBroadcast = prep
                                sentFragments = 0
                                broadcastState = BroadcastState.Ready(prep.fragmentPackets.size)
                            }

                            result.onFailure { e ->
                                broadcastState = BroadcastState.Error(e.message ?: "Unknown error")
                            }
                        } catch (e: Exception) {
                            broadcastState = BroadcastState.Error(e.message ?: "Failed to prepare")
                        }
                    }
                },
                onSimulateSend = {
                    currentBroadcast?.let { broadcast ->
                        broadcastState = BroadcastState.Broadcasting(0, broadcast.fragmentPackets.size)
                        sentFragments = 0

                        scope.launch(Dispatchers.IO) {
                            for ((index, _) in broadcast.fragmentPackets.withIndex()) {
                                delay(100) // Simulate transmission delay

                                sentFragments = index + 1
                                broadcastState = BroadcastState.Broadcasting(
                                    sentFragments,
                                    broadcast.fragmentPackets.size
                                )

                                // Log packet info
                                android.util.Log.d(
                                    "Broadcast",
                                    "Sent fragment ${index + 1}/${broadcast.fragmentPackets.size}"
                                )
                            }

                            delay(500)
                            broadcastState = BroadcastState.Complete
                        }
                    }
                },
                onReset = {
                    broadcastState = BroadcastState.Idle
                    currentBroadcast = null
                    sentFragments = 0
                }
            )
        }
    }
}

/**
 * Visual indicator of broadcast status
 */
@Composable
private fun BroadcastStatusIndicator(state: BroadcastState) {
    val (color, text, isAnimating) = when (state) {
        BroadcastState.Idle -> Triple(Color.Gray, "Ready to broadcast", false)
        BroadcastState.Preparing -> Triple(Color.Blue, "Preparing broadcast...", true)
        is BroadcastState.Ready -> Triple(Color.Green, "Ready (${state.fragmentCount} fragments)", false)
        is BroadcastState.Broadcasting -> {
            val progress = "${state.sent}/${state.total}"
            Triple(Color.Cyan, "Broadcasting $progress", true)
        }
        BroadcastState.Complete -> Triple(Color.Green, "✅ Broadcast complete!", false)
        is BroadcastState.Error -> Triple(Color.Red, "Error: ${state.message}", false)
    }
    
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                color.copy(alpha = 0.1f),
                RoundedCornerShape(8.dp)
            )
            .padding(12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        // Status indicator dot
        if (isAnimating) {
            PulsingDot(color)
        } else {
            Box(
                modifier = Modifier
                    .size(12.dp)
                    .background(color, CircleShape)
            )
        }
        
        Text(
            text = text,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
            color = color
        )
    }
}

/**
 * Animated pulsing dot
 */
@Composable
private fun PulsingDot(color: Color) {
    val infiniteTransition = rememberInfiniteTransition(label = "pulse")
    val alpha by infiniteTransition.animateFloat(
        initialValue = 1f,
        targetValue = 0.3f,
        animationSpec = infiniteRepeatable(
            animation = tween(600, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "alpha"
    )
    
    Box(
        modifier = Modifier
            .size(12.dp)
            .alpha(alpha)
            .background(color, CircleShape)
    )
}

/**
 * Transaction size selector
 */
@Composable
private fun TransactionSizeSelector(
    currentSize: Int,
    onSizeChanged: (Int) -> Unit,
    enabled: Boolean
) {
    Column {
        Text(
            text = "Test Transaction Size: $currentSize bytes",
            style = MaterialTheme.typography.bodySmall,
            color = if (enabled) MaterialTheme.colorScheme.onSurface 
                   else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.5f)
        )
        
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            SizeButton("Small\n(200B)", 200, currentSize, enabled, onSizeChanged)
            SizeButton("Typical\n(350B)", 350, currentSize, enabled, onSizeChanged)
            SizeButton("Large\n(800B)", 800, currentSize, enabled, onSizeChanged)
            SizeButton("Max\n(1232B)", 1232, currentSize, enabled, onSizeChanged)
        }
    }
}

@Composable
private fun RowScope.SizeButton(
    label: String,
    size: Int,
    currentSize: Int,
    enabled: Boolean,
    onSizeChanged: (Int) -> Unit
) {
    Button(
        onClick = { onSizeChanged(size) },
        enabled = enabled,
        colors = ButtonDefaults.buttonColors(
            containerColor = if (currentSize == size) 
                MaterialTheme.colorScheme.primary 
            else MaterialTheme.colorScheme.secondary
        ),
        modifier = Modifier
            .weight(1f)
            .height(56.dp)
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodySmall,
            fontWeight = if (currentSize == size) FontWeight.Bold else FontWeight.Normal
        )
    }
}

/**
 * Display broadcast information
 */
@Composable
private fun BroadcastInfo(
    broadcast: BroadcastPreparation,
    sentFragments: Int,
    state: BroadcastState
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                RoundedCornerShape(8.dp)
            )
            .padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        Text(
            text = "Broadcast Details",
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.Bold
        )
        
        InfoRow("TX ID", broadcast.transactionId.take(16) + "...")
        InfoRow("Fragments", "${broadcast.fragmentPackets.size} packets")
        InfoRow("Progress", "$sentFragments/${broadcast.fragmentPackets.size} sent")
        
        // Progress bar
        if (state is BroadcastState.Broadcasting || state is BroadcastState.Complete) {
            LinearProgressIndicator(
                progress = { sentFragments.toFloat() / broadcast.fragmentPackets.size },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(8.dp),
            )
        }
        
        // Fragment list
        if (broadcast.fragmentPackets.size <= 5) {
            Text(
                text = "Fragments:",
                style = MaterialTheme.typography.bodySmall,
                fontWeight = FontWeight.Medium
            )
            
            broadcast.fragmentPackets.forEachIndexed { index, packet ->
                val isSent = index < sentFragments
                val packetSize = try {
                    Base64.decode(packet.packetBytes, Base64.NO_WRAP).size
                } catch (e: Exception) {
                    0
                }
                
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(
                            if (isSent) Color.Green.copy(alpha = 0.1f)
                            else Color.Transparent,
                            RoundedCornerShape(4.dp)
                        )
                        .padding(4.dp),
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text(
                        text = "${if (isSent) "✓" else "○"} Fragment ${index + 1}",
                        style = MaterialTheme.typography.bodySmall,
                        color = if (isSent) Color.Green else MaterialTheme.colorScheme.onSurface
                    )
                    Text(
                        text = "$packetSize bytes",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.7f)
                    )
                }
            }
        }
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.7f)
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodySmall,
            fontWeight = FontWeight.Medium
        )
    }
}

/**
 * Control buttons
 */
@Composable
private fun BroadcastControls(
    state: BroadcastState,
    onPrepare: () -> Unit,
    onSimulateSend: () -> Unit,
    onReset: () -> Unit
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        when (state) {
            BroadcastState.Idle, is BroadcastState.Error -> {
                Button(
                    onClick = onPrepare,
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Prepare Broadcast")
                }
            }
            
            is BroadcastState.Ready -> {
                Button(
                    onClick = onSimulateSend,
                    modifier = Modifier.weight(1f),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.tertiary
                    )
                ) {
                    Text("Simulate Send")
                }
                OutlinedButton(
                    onClick = onReset,
                    modifier = Modifier.weight(0.5f)
                ) {
                    Text("Reset")
                }
            }
            
            BroadcastState.Complete -> {
                Button(
                    onClick = onReset,
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Broadcast Again")
                }
            }
            
            else -> {
                // Preparing or Broadcasting - show disabled button
                Button(
                    onClick = {},
                    enabled = false,
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Please wait...")
                }
            }
        }
    }
}

/**
 * Broadcast state
 */
sealed class BroadcastState {
    object Idle : BroadcastState()
    object Preparing : BroadcastState()
    data class Ready(val fragmentCount: Int) : BroadcastState()
    data class Broadcasting(val sent: Int, val total: Int) : BroadcastState()
    object Complete : BroadcastState()
    data class Error(val message: String) : BroadcastState()
}

