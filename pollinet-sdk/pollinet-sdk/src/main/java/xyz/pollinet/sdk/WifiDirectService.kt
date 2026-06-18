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
import android.net.wifi.WpsInfo
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
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlin.random.Random
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
        private const val CONFIRMATION_NOTIFICATION_ID = 1003
        private const val CHANNEL_ID = "pollinet_wifi_direct_service"

        const val ACTION_START = "xyz.pollinet.sdk.action.WIFI_START"
        const val ACTION_STOP = "xyz.pollinet.sdk.action.WIFI_STOP"
        const val EXTRA_HANDLE = "xyz.pollinet.sdk.extra.HANDLE"

        /** Connection role. AUTO = discover+connect (OS negotiates owner); OWNER = create
         *  an autonomous group and wait for clients; CLIENT = discover+connect only.
         *  Explicit OWNER/CLIENT roles avoid the symmetric-connect race that fails with
         *  ERROR/BUSY when both peers try to initiate at once. */
        const val EXTRA_ROLE = "xyz.pollinet.sdk.extra.ROLE"
        const val ROLE_AUTO = 0
        const val ROLE_OWNER = 1
        const val ROLE_CLIENT = 2

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

        /** How often to actively poll P2P connection state (ms). Belt-and-suspenders for
         *  devices that drop the WIFI_P2P_CONNECTION_CHANGED broadcast (e.g. some MTK). */
        private const val INFO_POLL_MS = 3_000L

        /** Reset a stalled connect attempt after this long with no group, so the client
         *  retries instead of getting stuck on a connect() that was accepted but never
         *  completed. */
        private const val CONNECT_TIMEOUT_MS = 12_000L

        // ── discover-then-elect + latch (ROLE_AUTO) tuning ──────────────────────
        /** Base time to scan for an existing owner before electing self as owner. */
        private const val ELECT_DISCOVER_MIN_MS = 4_000L
        /** Random extra scan time. JITTER is the whole point: two lonely devices must
         *  fall out of lockstep so one is discovering while the other owns → they pair. */
        private const val ELECT_DISCOVER_JITTER_MS = 5_000L
        /** Base time to hold an owned-but-empty group before relinquishing to re-discover. */
        private const val OWNER_HOLD_MIN_MS = 5_000L
        /** Random extra owner-hold time (jitter, same anti-lockstep reason). */
        private const val OWNER_HOLD_JITTER_MS = 5_000L
        /** Poll cadence while latched (connected) — just watching for a drop. */
        private const val LATCH_POLL_MS = 2_000L

        /** Coarse, process-wide Wi-Fi Direct link status for the UI to observe. */
        enum class LinkStatus { IDLE, DISCOVERING, OWNER_WAITING, CONNECTED }

        private val _linkStatus = MutableStateFlow(LinkStatus.IDLE)
        /** Observable link status (group role / connection phase). */
        val linkStatus: StateFlow<LinkStatus> = _linkStatus.asStateFlow()

        private val _connectedPeers = MutableStateFlow(0)
        /** Observable count of peers with an open socket. */
        val connectedPeers: StateFlow<Int> = _connectedPeers.asStateFlow()

        // ── Wire frame types (1-byte tag prefixed to every socket payload) ──────
        /** Forward path: a bincode TransactionFragment → engine `pushInbound`. */
        const val FRAME_TYPE_FRAGMENT: Byte = 0x00
        /** Reverse path: a JSON Confirmation → engine confirmation handling. */
        const val FRAME_TYPE_CONFIRMATION: Byte = 0x01

        /** Stop relaying a confirmation once it has hopped this many times. */
        private const val MAX_CONFIRMATION_RELAY_HOPS = 3

        /** A confirmation surfaced from the Wi-Fi reverse-channel (for the UI). */
        data class ConfirmationInfo(val txIdShort: String, val success: Boolean, val detail: String)

        private val _confirmations = MutableSharedFlow<ConfirmationInfo>(extraBufferCapacity = 8)
        /** Observable stream of confirmations received over Wi-Fi Direct. */
        val confirmations: SharedFlow<ConfirmationInfo> = _confirmations.asSharedFlow()
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    /** JSON codec for the confirmation reverse-channel (matches the SDK's Confirmation shape). */
    private val cjson = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    /** Echo-loop guard: confirmations bounce between paired devices; first receive of a txId
     *  wins, later echoes are dropped. Bounded LRU so it can't grow unbounded. */
    private val seenConfirmationTxIds = java.util.Collections.synchronizedSet(
        object : LinkedHashSet<String>() {
            override fun add(element: String): Boolean {
                if (size >= 256) iterator().let { if (it.hasNext()) { it.next(); it.remove() } }
                return super.add(element)
            }
        }
    )

    private lateinit var manager: WifiP2pManager
    private lateinit var channel: WifiP2pManager.Channel
    private var receiver: BroadcastReceiver? = null

    /** Rust transport handle (a Wi-Fi Direct handle from initWifiDirect/Sharing). */
    @Volatile private var handle: Long = -1

    @Volatile private var isGroupOwner = false
    @Volatile private var ownerAddress: String? = null

    /** True while a single connect() attempt is outstanding — guards against the
     *  connect() storm that fires on every PEERS_CHANGED broadcast and prevents the
     *  group from ever stabilizing. */
    @Volatile private var connecting = false
    /** True once we've begun establishing the socket session for the current group,
     *  so repeated CONNECTION_CHANGED broadcasts don't re-open sockets. */
    @Volatile private var sessionActive = false

    private var serverSocket: ServerSocket? = null

    /** One entry per connected peer. Each peer owns a private outbound channel so a slow
     *  link backs up only itself — never the broadcast loop or the other peers. */
    private val peers = CopyOnWriteArrayList<Peer>()
    private var ioJobs = mutableListOf<Job>()
    private var discoveryJob: Job? = null
    private var broadcastJob: Job? = null
    private var infoPollJob: Job? = null
    private var electJob: Job? = null
    /** SystemClock timestamp of the last connect() request; 0 when not connecting. */
    @Volatile private var connectStartedAt = 0L
    /** The role this session was started with (drives reconnect behavior). */
    @Volatile private var currentRole = ROLE_AUTO

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
                currentRole = intent?.getIntExtra(EXTRA_ROLE, ROLE_AUTO) ?: ROLE_AUTO
                startForegroundCompat()
                registerP2pReceiver()
                startBroadcastLoop()
                startConnectionInfoPoller()
                // Cancel any prior role loops so a role switch starts clean.
                electJob?.cancel(); discoveryJob?.cancel()
                when (currentRole) {
                    // Group owner: create an autonomous group and wait for clients to join.
                    // No discovery — the client finds us. onConnectionInfo opens the server.
                    ROLE_OWNER -> { _linkStatus.value = LinkStatus.OWNER_WAITING; createOwnerGroup() }
                    // Explicit client: discover peers and connect to the (only) one found.
                    ROLE_CLIENT -> { _linkStatus.value = LinkStatus.DISCOVERING; startDiscoveryLoop() }
                    // Auto (default): discover-then-elect + latch + jittered window.
                    else -> startDiscoverThenElect()
                }
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
                // Forward path: a tx fragment, if any.
                val fragment = PolliNetFFI.nextOutbound(handle, MAX_PAYLOAD.toLong())
                if (fragment != null) {
                    idle = WRITER_IDLE_BACKOFF_MS
                    fanOut(FRAME_TYPE_FRAGMENT, fragment)
                    log("tx fragment ${fragment.size}B → ${peers.size} peer(s)")
                    continue
                }
                // Reverse path: a confirmation, if any (the engine's confirmation queue).
                val confirmation = popConfirmationFrame()
                if (confirmation != null) {
                    idle = WRITER_IDLE_BACKOFF_MS
                    fanOut(FRAME_TYPE_CONFIRMATION, confirmation)
                    log("tx confirmation ${confirmation.size}B → ${peers.size} peer(s)")
                    continue
                }
                // Nothing to send — back off (no busy-spin).
                delay(idle)
                idle = (idle * 2).coerceAtMost(WRITER_IDLE_BACKOFF_MAX_MS)
            }
        }
    }

    // ── Confirmation reverse-channel ───────────────────────────────────────────

    /** Prefix [payload] with the 1-byte frame [type] and fan it to every connected peer. */
    private fun fanOut(type: Byte, payload: ByteArray) {
        val framed = ByteArray(payload.size + 1)
        framed[0] = type
        System.arraycopy(payload, 0, framed, 1, payload.size)
        for (p in peers) p.outbound.trySend(framed)
    }

    /** Pop one confirmation from the shared engine queue and return its JSON bytes, or null.
     *  Works on the Wi-Fi handle because the shared handle now exposes the BLE engine surface. */
    private fun popConfirmationFrame(): ByteArray? {
        if (handle < 0) return null
        val resultJson = runCatching { PolliNetFFI.popConfirmation(handle) }.getOrNull() ?: return null
        val conf = runCatching {
            cjson.decodeFromString(PopConfirmationResult.serializer(), resultJson)
        }.getOrNull()?.takeIf { it.ok }?.data ?: return null
        return runCatching {
            cjson.encodeToString(Confirmation.serializer(), conf).toByteArray(Charsets.UTF_8)
        }.getOrNull()
    }

    /** Handle a confirmation received over Wi-Fi: purge the origin's tx, surface UI, relay on. */
    private fun handleWifiConfirmation(payload: ByteArray) {
        val conf = runCatching {
            cjson.decodeFromString(Confirmation.serializer(), String(payload, Charsets.UTF_8))
        }.getOrNull() ?: run { log("confirmation parse failed (${payload.size}B)"); return }

        // Echo guard — confirmations ping-pong between paired peers; first receive wins.
        if (!seenConfirmationTxIds.add(conf.txId)) {
            log("confirmation ${conf.txId.take(8)}… already seen — skipping echo")
            return
        }
        val txShort = conf.txId.take(8)

        // Purge: stop the origin re-broadcasting this now-confirmed tx (store-and-forward off).
        if (handle >= 0) runCatching { PolliNetFFI.confirmDelivered(handle, conf.txId) }

        // Surface to the UI (observable event + system notification).
        val (success, detail) = when (val s = conf.status) {
            is ConfirmationStatus.Success -> true to s.signature.take(16)
            is ConfirmationStatus.Failed -> false to s.error
        }
        _confirmations.tryEmit(ConfirmationInfo(txShort, success, detail))
        postConfirmationNotification(
            if (success) "Transaction Confirmed ✓" else "Transaction Failed",
            if (success) "Tx $txShort… confirmed on Solana" else "Tx $txShort… failed: $detail",
        )
        log("confirmation $txShort success=$success — purged + surfaced")

        // Relay onward through the mesh until the hop cap is reached.
        if (conf.relayCount < MAX_CONFIRMATION_RELAY_HOPS && handle >= 0) {
            runCatching {
                PolliNetFFI.relayConfirmation(handle, cjson.encodeToString(Confirmation.serializer(), conf))
            }
        }
    }

    private fun postConfirmationNotification(title: String, text: String) {
        val nm = getSystemService(NotificationManager::class.java) ?: return
        val n = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(title)
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()
        nm.notify(CONFIRMATION_NOTIFICATION_ID, n)
    }

    // ── Discovery (battery-aware: windowed, not continuous) ──────────────────

    private fun startDiscoveryLoop() {
        discoveryJob?.cancel()
        discoveryJob = scope.launch {
            while (isActive) {
                // Only scan when idle: no connected peers, and no connect/session in
                // progress. Re-discovering during a connect throws BUSY and breaks the join.
                if (peers.isEmpty() && !connecting && !sessionActive) {
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

    /**
     * Actively poll P2P connection state instead of trusting the
     * `WIFI_P2P_CONNECTION_CHANGED` broadcast alone — some devices (notably MTK) drop it,
     * so the group owner would otherwise never learn the group formed and never open its
     * server socket. Also resets a stalled client connect so discovery can retry.
     */
    @Suppress("MissingPermission")
    private fun startConnectionInfoPoller() {
        infoPollJob?.cancel()
        infoPollJob = scope.launch {
            while (isActive) {
                if (!sessionActive) {
                    manager.requestConnectionInfo(channel) { info ->
                        if (info.groupFormed) onConnectionInfo(info)
                    }
                    // Retry a connect that was accepted but never produced a group.
                    if (connecting && connectStartedAt > 0L &&
                        android.os.SystemClock.elapsedRealtime() - connectStartedAt > CONNECT_TIMEOUT_MS
                    ) {
                        log("connect stalled — resetting to retry")
                        connecting = false
                        connectStartedAt = 0L
                    }
                }
                delay(INFO_POLL_MS)
            }
        }
    }

    /**
     * Become an autonomous group owner. Removes any stale group first, then creates a
     * fresh one; [onConnectionInfo] fires with `isGroupOwner = true` and opens the server
     * socket. Deterministic counterpart to a [ROLE_CLIENT] peer — no connect race.
     */
    @Suppress("MissingPermission")
    private fun createOwnerGroup() {
        // Clear any leftover group from a previous session so createGroup doesn't fail BUSY.
        manager.removeGroup(channel, object : WifiP2pManager.ActionListener {
            override fun onSuccess() { doCreateGroup() }
            override fun onFailure(reason: Int) { doCreateGroup() } // none existed — fine
        })
    }

    @Suppress("MissingPermission")
    private fun doCreateGroup() {
        manager.createGroup(channel, object : WifiP2pManager.ActionListener {
            override fun onSuccess() { log("createGroup ok — awaiting clients as group owner") }
            override fun onFailure(reason: Int) { log("createGroup failed: $reason") }
        })
    }

    /**
     * ROLE_AUTO strategy: **discover-then-elect + latch**, with a **jittered** window.
     *
     * Each cycle (only while not connected):
     *  1. Discover for a randomized window. If a peer appears, [onPeersAvailable] connects us
     *     as a client → we latch.
     *  2. If nobody showed up, elect self as owner ([createOwnerGroup]) and hold for a
     *     randomized window so a client can join → latch.
     *  3. If still nobody joined, **relinquish** the group and loop back to discovery.
     *
     * The jitter is essential: it desyncs two lonely devices so one is discovering while the
     * other owns, which is the only way they ever pair (owners don't discover each other).
     * Once a peer socket is up (`connectedPeers > 0`) the loop idles — that's the latch.
     */
    @Suppress("MissingPermission")
    private fun startDiscoverThenElect() {
        electJob?.cancel()
        electJob = scope.launch {
            while (isActive) {
                // Latched: a peer socket is up. Stay put; just watch for a drop.
                if (connectedPeers.value > 0) {
                    delay(LATCH_POLL_MS)
                    continue
                }

                // ── Phase 1: discover ──
                _linkStatus.value = LinkStatus.DISCOVERING
                discoverOnce()
                val discoverDeadline = android.os.SystemClock.elapsedRealtime() +
                    ELECT_DISCOVER_MIN_MS + Random.nextLong(ELECT_DISCOVER_JITTER_MS)
                while (isActive && connectedPeers.value == 0 && !connecting &&
                    android.os.SystemClock.elapsedRealtime() < discoverDeadline
                ) {
                    delay(400)
                }
                if (connectedPeers.value > 0) continue          // joined a peer → latch
                if (connecting) {                                // connect in flight → let it resolve
                    delay(CONNECT_TIMEOUT_MS + 1_500)
                    continue
                }

                // ── Phase 2: elect self as owner ──
                _linkStatus.value = LinkStatus.OWNER_WAITING
                createOwnerGroup()
                val ownerDeadline = android.os.SystemClock.elapsedRealtime() +
                    OWNER_HOLD_MIN_MS + Random.nextLong(OWNER_HOLD_JITTER_MS)
                while (isActive && connectedPeers.value == 0 &&
                    android.os.SystemClock.elapsedRealtime() < ownerDeadline
                ) {
                    delay(500)
                }
                if (connectedPeers.value > 0) continue          // a client joined → latch

                // ── Phase 3: no client joined — relinquish so two lonely owners can pair ──
                log("no clients joined as owner — relinquishing group to re-discover")
                relinquishGroup()
            }
        }
    }

    /** Tear down an owned-but-empty group and reset session state so the elect loop can
     *  fall back to discovery (split-brain resolution). */
    @Suppress("MissingPermission")
    private fun relinquishGroup() {
        runCatching { manager.removeGroup(channel, null) }
        isGroupOwner = false
        ownerAddress = null
        sessionActive = false
        connecting = false
        connectStartedAt = 0L
    }

    @Suppress("MissingPermission")
    private fun onPeersAvailable(devices: Collection<WifiP2pDevice>) {
        // Connect exactly once. PEERS_CHANGED fires repeatedly during discovery; without
        // this guard every broadcast triggers a fresh connect(), which storms the P2P
        // framework with BUSY errors and stops the group from ever forming.
        if (connecting || sessionActive || peers.isNotEmpty()) return
        val device = devices.firstOrNull() ?: return
        connecting = true
        connectStartedAt = android.os.SystemClock.elapsedRealtime()
        // IMPORTANT: do NOT stopPeerDiscovery() here. Stopping discovery makes the framework
        // immediately mark the just-found peer as LOST, and the connect() that follows is
        // then dropped ("Dropping connect request"). Connect straight from the fresh
        // discovery result; the framework stops discovery itself as part of connecting.
        val config = WifiP2pConfig().apply {
            deviceAddress = device.deviceAddress
            wps.setup = WpsInfo.PBC
            // We are the client; intent 0 = "let the discovered peer (the owner) be GO".
            groupOwnerIntent = 0
        }
        log("connecting to ${device.deviceAddress} as client (GO-intent 0)")
        manager.connect(channel, config, object : WifiP2pManager.ActionListener {
            override fun onSuccess() { log("connect() to ${device.deviceAddress} requested") }
            override fun onFailure(reason: Int) {
                log("connect() failed: $reason")
                connecting = false // allow a fresh attempt on the next peer update
                connectStartedAt = 0L
            }
        })
    }

    private fun onConnectionInfo(info: WifiP2pInfo) {
        if (!info.groupFormed) {
            connecting = false
            return
        }
        connecting = false
        connectStartedAt = 0L
        if (sessionActive) return // sockets already being set up for this group
        sessionActive = true
        discoveryJob?.cancel() // connected — stop the discovery loop
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
        _connectedPeers.value = peers.size
        _linkStatus.value = LinkStatus.CONNECTED
        log("peer connected (${peers.size} total)")
        val input = DataInputStream(socket.getInputStream())

        // Reader: length-prefixed frames → pushInbound.
        ioJobs += scope.launch {
            try {
                while (isActive && !socket.isClosed) {
                    val len = input.readInt() // 4-byte BE length prefix (covers 1B type + payload)
                    if (len < 1 || len > MAX_FRAME) {
                        log("bad frame length $len → dropping peer")
                        break
                    }
                    val type = input.readByte()
                    val payload = ByteArray(len - 1)
                    input.readFully(payload)
                    if (handle < 0) continue
                    when (type) {
                        FRAME_TYPE_FRAGMENT -> {
                            log("rx fragment ${payload.size}B")
                            PolliNetFFI.pushInbound(handle, payload)
                        }
                        FRAME_TYPE_CONFIRMATION -> {
                            log("rx confirmation ${payload.size}B")
                            handleWifiConfirmation(payload)
                        }
                        else -> log("rx unknown frame type $type (${payload.size}B) — ignored")
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
        connecting = false
        sessionActive = false
        connectStartedAt = 0L
        _connectedPeers.value = 0
        _linkStatus.value = LinkStatus.DISCOVERING
        // Re-establish per role. The persisted outbound queue re-sends on the next session.
        when (currentRole) {
            ROLE_OWNER -> { _linkStatus.value = LinkStatus.OWNER_WAITING; createOwnerGroup() }
            ROLE_CLIENT -> startDiscoveryLoop()
            // Auto: the elect loop is persistent and will re-cycle now that peers==0.
            // Restart it only if it somehow died.
            else -> if (electJob?.isActive != true) startDiscoverThenElect()
        }
    }

    private fun closePeer(peer: Peer) {
        peer.outbound.close()
        runCatching { peer.socket.close() }
        peers.remove(peer)
        _connectedPeers.value = peers.size
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
        infoPollJob?.cancel()
        electJob?.cancel()
        closeAllSockets()
        receiver?.let { runCatching { unregisterReceiver(it) } }
        receiver = null
        runCatching { manager.removeGroup(channel, null) }
        _connectedPeers.value = 0
        _linkStatus.value = LinkStatus.IDLE
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

/** Parses the FfiResult returned by [PolliNetFFI.popConfirmation] — `{ ok, data }` where
 *  data is the [Confirmation] (or null when the queue is empty / on error). */
@Serializable
private class PopConfirmationResult(
    val ok: Boolean = false,
    val data: Confirmation? = null,
)
