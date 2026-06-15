package xyz.pollinet.sdk

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.net.wifi.p2p.WifiP2pConfig
import android.net.wifi.p2p.WifiP2pDevice
import android.net.wifi.p2p.WifiP2pInfo
import android.net.wifi.p2p.WifiP2pManager
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.CopyOnWriteArrayList

/**
 * Foreground service for Wi-Fi Direct (Wi-Fi P2P) transport.
 *
 * Mirrors [BleService]'s shape: it owns the radio (discovery, group formation, sockets)
 * and bridges raw frames to the Rust core via the *same* host-driven transport contract
 * ([PolliNetFFI.pushInbound] / [PolliNetFFI.nextOutbound]). It knows nothing about
 * routing, voting, polling, or Solana semantics — those live in the Rust shared layers.
 *
 * Framing on the socket is **length-prefixed** (4-byte big-endian length + payload),
 * where each payload is exactly the bincode `TransactionFragment` the Rust engine emits.
 *
 * NOTE: requires `ACCESS_FINE_LOCATION` (and `NEARBY_WIFI_DEVICES` on API 33+),
 * `ACCESS_WIFI_STATE`, `CHANGE_WIFI_STATE`, `INTERNET` permissions in the host app
 * manifest, plus a `<service android:foregroundServiceType="connectedDevice">` entry.
 */
class WifiDirectService : Service() {

    companion object {
        private const val NOTIFICATION_ID = 1002
        private const val CHANNEL_ID = "pollinet_wifi_direct_service"

        const val ACTION_START = "xyz.pollinet.sdk.action.WIFI_START"
        const val ACTION_STOP = "xyz.pollinet.sdk.action.WIFI_STOP"
        const val EXTRA_HANDLE = "xyz.pollinet.sdk.extra.HANDLE"

        /** TCP port the group owner listens on. */
        const val SOCKET_PORT = 8988

        /** Must match Rust `WIFI_DIRECT_MAX_FRAME` (DoS guard for the length prefix). */
        const val MAX_FRAME = 16 * 1024

        /** Must match Rust `WIFI_DIRECT_MAX_PAYLOAD` — the per-frame budget we request. */
        const val MAX_PAYLOAD = 1400

        // Battery: discovery is *not* continuous. Discover for a window, idle, repeat.
        private const val DISCOVERY_IDLE_MS = 30_000L
        private const val WRITER_IDLE_BACKOFF_MS = 50L
        private const val WRITER_IDLE_BACKOFF_MAX_MS = 500L

        /** Per-peer outbound backlog cap. A slow/dead peer drops its own oldest frames
         *  (DROP_OLDEST) instead of stalling the broadcast loop or other peers. */
        private const val OUTBOUND_PER_PEER_CAPACITY = 256
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private lateinit var manager: WifiP2pManager
    private lateinit var channel: WifiP2pManager.Channel
    private var receiver: BroadcastReceiver? = null

    /** Rust transport handle (a Wi-Fi Direct handle from initWifiDirect/Sharing). */
    @Volatile private var handle: Long = -1

    @Volatile private var isGroupOwner = false
    @Volatile private var ownerAddress: String? = null

    private var serverSocket: ServerSocket? = null

    /** One entry per connected peer. Each peer owns a private outbound channel so a slow
     *  link backs up only itself — never the broadcast loop or the other peers. */
    private val peers = CopyOnWriteArrayList<Peer>()
    private var ioJobs = mutableListOf<Job>()
    private var discoveryJob: Job? = null
    private var broadcastJob: Job? = null

    /** A connected peer: its socket, framed output stream, and private outbound backlog. */
    private class Peer(
        val socket: Socket,
        val output: DataOutputStream,
        val outbound: Channel<ByteArray>,
    )

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        manager = getSystemService(Context.WIFI_P2P_SERVICE) as WifiP2pManager
        channel = manager.initialize(this, mainLooper, null)
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopEverything()
                stopSelf()
                return START_NOT_STICKY
            }
            else -> {
                handle = intent?.getLongExtra(EXTRA_HANDLE, -1) ?: -1
                startForegroundCompat()
                registerP2pReceiver()
                startBroadcastLoop()
                startDiscoveryLoop()
            }
        }
        return START_STICKY
    }

    // ── Outbound broadcast (single drainer, per-peer fan-out) ─────────────────

    /**
     * The one and only consumer of the Rust outbound queue.
     *
     * A mesh frame must reach *every* connected peer, but [PolliNetFFI.nextOutbound]
     * removes the frame from the engine queue when polled. If each peer's writer polled
     * it independently, the peers would race and each frame would land on exactly one
     * peer. So instead a single loop polls once and fans the frame into every peer's
     * private channel; each [Peer.outbound] is a DROP_OLDEST buffer, so a slow/dead link
     * sheds its *own* oldest frames without stalling the drainer or the other peers.
     *
     * Store-and-forward is preserved: while no peer can receive (`peers` empty or no
     * handle), the loop idles and never drains the queue, so the Rust engine keeps the
     * frames until a session is up.
     */
    private fun startBroadcastLoop() {
        broadcastJob?.cancel()
        broadcastJob = scope.launch {
            var idle = WRITER_IDLE_BACKOFF_MS
            while (isActive) {
                // Don't drain the engine queue when nobody can receive — store-and-forward.
                if (handle < 0 || peers.isEmpty()) {
                    delay(idle)
                    idle = (idle * 2).coerceAtMost(WRITER_IDLE_BACKOFF_MAX_MS)
                    continue
                }
                val frame = PolliNetFFI.nextOutbound(handle, MAX_PAYLOAD.toLong())
                if (frame == null) {
                    delay(idle)
                    idle = (idle * 2).coerceAtMost(WRITER_IDLE_BACKOFF_MAX_MS)
                    continue
                }
                idle = WRITER_IDLE_BACKOFF_MS
                // Fan out to every peer; DROP_OLDEST means a full channel drops its own
                // oldest frame, never blocking this loop or another peer.
                for (p in peers) {
                    p.outbound.trySend(frame)
                }
            }
        }
    }

    // ── Discovery (battery-aware: windowed, not continuous) ──────────────────

    private fun startDiscoveryLoop() {
        discoveryJob?.cancel()
        discoveryJob = scope.launch {
            while (isActive) {
                if (peers.isEmpty()) {
                    discoverOnce()
                }
                delay(DISCOVERY_IDLE_MS)
            }
        }
    }

    @Suppress("MissingPermission")
    private fun discoverOnce() {
        manager.discoverPeers(channel, object : WifiP2pManager.ActionListener {
            override fun onSuccess() { log("discoverPeers started") }
            override fun onFailure(reason: Int) { log("discoverPeers failed: $reason") }
        })
    }

    @Suppress("MissingPermission")
    private fun onPeersAvailable(peers: Collection<WifiP2pDevice>) {
        // Connect to the first available peer. The OS negotiates the group owner.
        val peer = peers.firstOrNull() ?: return
        val config = WifiP2pConfig().apply { deviceAddress = peer.deviceAddress }
        manager.connect(channel, config, object : WifiP2pManager.ActionListener {
            override fun onSuccess() { log("connect() to ${peer.deviceAddress} ok") }
            override fun onFailure(reason: Int) { log("connect() failed: $reason") }
        })
    }

    private fun onConnectionInfo(info: WifiP2pInfo) {
        if (!info.groupFormed) return
        isGroupOwner = info.isGroupOwner
        ownerAddress = info.groupOwnerAddress?.hostAddress
        log("group formed: owner=$isGroupOwner ownerAddr=$ownerAddress")
        if (isGroupOwner) startServer() else startClient()
    }

    // ── Sockets ──────────────────────────────────────────────────────────────

    private fun startServer() {
        scope.launch {
            try {
                val ss = ServerSocket(SOCKET_PORT)
                serverSocket = ss
                log("ServerSocket open on $SOCKET_PORT")
                while (isActive && !ss.isClosed) {
                    val socket = ss.accept() // blocks until a client connects
                    log("client connected: ${socket.inetAddress}")
                    bindSocket(socket)
                }
            } catch (e: Exception) {
                log("server socket error: ${e.message}")
                onLinkDown()
            }
        }
    }

    private fun startClient() {
        scope.launch {
            val addr = ownerAddress ?: return@launch
            try {
                val socket = Socket()
                socket.bind(null)
                socket.connect(InetSocketAddress(addr, SOCKET_PORT), 10_000)
                log("connected to owner $addr")
                bindSocket(socket)
            } catch (e: Exception) {
                log("client socket error: ${e.message}")
                onLinkDown()
            }
        }
    }

    /** Register a connected socket as a [Peer] and attach its reader + writer loops. */
    private fun bindSocket(socket: Socket) {
        val output = DataOutputStream(socket.getOutputStream())
        val outbound = Channel<ByteArray>(
            capacity = OUTBOUND_PER_PEER_CAPACITY,
            onBufferOverflow = BufferOverflow.DROP_OLDEST,
        )
        val peer = Peer(socket, output, outbound)
        peers.add(peer)
        val input = DataInputStream(socket.getInputStream())

        // Reader: length-prefixed frames → pushInbound.
        ioJobs += scope.launch {
            try {
                while (isActive && !socket.isClosed) {
                    val len = input.readInt() // 4-byte big-endian length prefix
                    if (len <= 0 || len > MAX_FRAME) {
                        log("bad frame length $len → dropping peer")
                        break
                    }
                    val payload = ByteArray(len)
                    input.readFully(payload)
                    if (handle >= 0) {
                        PolliNetFFI.pushInbound(handle, payload)
                    }
                }
            } catch (e: Exception) {
                log("reader closed: ${e.message}")
            } finally {
                closePeer(peer)
            }
        }

        // Writer: drain *this peer's* private channel and write length-prefixed frames.
        // Suspends on an empty channel (no busy-spin); the broadcast loop is the sole
        // producer, so there is no cross-peer race on the engine queue.
        ioJobs += scope.launch {
            try {
                for (frame in outbound) {
                    output.writeInt(frame.size)
                    output.write(frame)
                    output.flush()
                }
            } catch (e: Exception) {
                log("writer closed: ${e.message}")
            } finally {
                closePeer(peer)
            }
        }
    }

    // ── Lifecycle / reconnect ─────────────────────────────────────────────────

    /**
     * A link went down (socket reset, group owner left). Tear down sockets but KEEP the
     * Rust queues — store-and-forward means the next session resumes, not restarts.
     */
    private fun onLinkDown() {
        log("link down → tearing down sockets, queues preserved")
        closeAllSockets()
        isGroupOwner = false
        ownerAddress = null
        // Re-enter discovery; the persisted outbound queue re-sends on the next session.
        startDiscoveryLoop()
    }

    private fun closePeer(peer: Peer) {
        peer.outbound.close()
        runCatching { peer.socket.close() }
        peers.remove(peer)
        if (peers.isEmpty()) onLinkDown()
    }

    private fun closeAllSockets() {
        ioJobs.forEach { it.cancel() }
        ioJobs.clear()
        peers.forEach { p ->
            p.outbound.close()
            runCatching { p.socket.close() }
        }
        peers.clear()
        runCatching { serverSocket?.close() }
        serverSocket = null
    }

    private fun stopEverything() {
        discoveryJob?.cancel()
        closeAllSockets()
        receiver?.let { runCatching { unregisterReceiver(it) } }
        receiver = null
        runCatching { manager.removeGroup(channel, null) }
        scope.cancel()
    }

    override fun onDestroy() {
        stopEverything()
        super.onDestroy()
    }

    // ── Broadcast receiver wiring ──────────────────────────────────────────────

    private fun registerP2pReceiver() {
        val filter = IntentFilter().apply {
            addAction(WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION)
            addAction(WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION)
            addAction(WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION)
        }
        val r = object : BroadcastReceiver() {
            @Suppress("MissingPermission")
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.action) {
                    WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION ->
                        manager.requestPeers(channel) { peers -> onPeersAvailable(peers.deviceList) }
                    WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION ->
                        manager.requestConnectionInfo(channel) { info -> onConnectionInfo(info) }
                }
            }
        }
        receiver = r
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(r, filter, RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            registerReceiver(r, filter)
        }
    }

    // ── Foreground notification ────────────────────────────────────────────────

    private fun startForegroundCompat() {
        val notification: Notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("PolliNet Wi-Fi Direct")
            .setContentText("Mesh transport active")
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setOngoing(true)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val mgr = getSystemService(NotificationManager::class.java)
            val ch = NotificationChannel(
                CHANNEL_ID, "PolliNet Wi-Fi Direct", NotificationManager.IMPORTANCE_LOW
            )
            mgr.createNotificationChannel(ch)
        }
    }

    private fun log(msg: String) = android.util.Log.d("PolliNet-WifiDirect", msg)
}
