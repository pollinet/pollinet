import Foundation
import CoreBluetooth
import Combine

// MARK: - Constants

private enum BLE {
    static let serviceUUID       = CBUUID(string: "00001820-0000-1000-8000-00805f9b34fb")
    static let txCharUUID        = CBUUID(string: "00001821-0000-1000-8000-00805f9b34fb")
    static let rxCharUUID        = CBUUID(string: "00001822-0000-1000-8000-00805f9b34fb")
    static let cccdUUID          = CBUUID(string: "00002902-0000-1000-8000-00805f9b34fb")

    static let defaultMtu            = 185
    static let idleDisconnectMs: Int = 4_000
    static let peerCooldownMs: Int   = 45_000
    static let maxTxRelayHops        = 5
}

// MARK: - Peer record

private struct Peer {
    let peripheral: CBPeripheral
    var txChar: CBCharacteristic?
    var connectedAt: Date = .now
    var lastFrameAt: Date = .now
}

// MARK: - BLE event bridge → SdkManager

protocol BleManagerDelegate: AnyObject {
    /// Raw BLE frame received from a remote node.
    func bleManager(_ manager: BleManager, didReceiveFrame data: Data)
    /// A node connected — flush any queued outbound confirmations.
    func bleManagerDidConnect(_ manager: BleManager)
    /// A node disconnected.
    func bleManagerDidDisconnect(_ manager: BleManager)
}

// MARK: - BleManager

/// Dual-role (central + peripheral) mesh node.
///
/// Role alternation strategy:
///   • Starts advertising to receive inbound traffic.
///   • While advertising also scans; on discovery connects as central.
///   • After idle window (4 s) with no frames, disconnects and re-scans.
///   • After disconnect, respects a 45 s peer cooldown before reconnecting the same peer.
@MainActor
final class BleManager: NSObject, ObservableObject {

    // MARK: Published state
    @Published private(set) var isAdvertising = false
    @Published private(set) var isScanning    = false
    @Published private(set) var connectedPeers: [String] = []
    @Published private(set) var log: [String] = []

    // MARK: Delegate
    weak var delegate: BleManagerDelegate?

    // MARK: Private — central
    private var central: CBCentralManager!
    private var peers: [UUID: Peer] = [:]
    private var peerCooldown: [UUID: Date] = [:]

    // MARK: Private — peripheral
    private var peripheral: CBPeripheralManager!
    private var rxChar: CBMutableCharacteristic?
    private var subscribedCentrals: [CBCentral] = []
    private var outboundBuffer: [Data] = []

    // MARK: Private — idle timer
    private var idleTimers: [UUID: Task<Void, Never>] = [:]

    // MARK: Init

    override init() {
        super.init()
        central    = CBCentralManager(delegate: self, queue: nil,
                                      options: [CBCentralManagerOptionRestoreIdentifierKey: "pollinet-central"])
        peripheral = CBPeripheralManager(delegate: self, queue: nil,
                                         options: [CBPeripheralManagerOptionRestoreIdentifierKey: "pollinet-peripheral"])
    }

    // MARK: Public

    /// Send a frame to all connected peers (central role → write to TX char; peripheral role → notify subscribers).
    func send(_ data: Data) {
        // Central role: write to connected peers' TX characteristic
        for (_, peer) in peers {
            guard let txChar = peer.txChar else { continue }
            peer.peripheral.writeValue(data, for: txChar, type: .withoutResponse)
        }
        // Peripheral role: notify subscribed centrals via RX characteristic
        if let rxChar, !subscribedCentrals.isEmpty {
            peripheral.updateValue(data, for: rxChar, onSubscribedCentrals: nil)
        }
        // Buffer for when no one is connected yet
        if peers.isEmpty && subscribedCentrals.isEmpty {
            outboundBuffer.append(data)
        }
    }

    // MARK: Private helpers

    private func startAdvertising() {
        guard peripheral.state == .poweredOn, !isAdvertising else { return }
        let advertisementData: [String: Any] = [
            CBAdvertisementDataServiceUUIDsKey: [BLE.serviceUUID],
            CBAdvertisementDataLocalNameKey: "PolliNet"
        ]
        peripheral.startAdvertising(advertisementData)
    }

    private func startScanning() {
        guard central.state == .poweredOn, !isScanning else { return }
        central.scanForPeripherals(withServices: [BLE.serviceUUID],
                                   options: [CBCentralManagerScanOptionAllowDuplicatesKey: false])
        isScanning = true
        appendLog("Scanning…")
    }

    private func connectPeer(_ cbPeripheral: CBPeripheral) {
        if let cooldownUntil = peerCooldown[cbPeripheral.identifier], Date() < cooldownUntil { return }
        guard peers[cbPeripheral.identifier] == nil else { return }
        central.connect(cbPeripheral, options: nil)
        appendLog("Connecting to \(cbPeripheral.identifier.uuidString.prefix(8))…")
    }

    private func disconnectPeer(_ id: UUID) {
        guard let peer = peers[id] else { return }
        central.cancelPeripheralConnection(peer.peripheral)
        idleTimers[id]?.cancel()
        idleTimers[id] = nil
    }

    private func scheduleIdleDisconnect(for id: UUID) {
        idleTimers[id]?.cancel()
        idleTimers[id] = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(BLE.idleDisconnectMs) * 1_000_000)
            guard !Task.isCancelled else { return }
            await self?.disconnectPeer(id)
        }
    }

    private func refreshLastFrame(for id: UUID) {
        peers[id]?.lastFrameAt = .now
        scheduleIdleDisconnect(for: id)
    }

    private func flushOutboundBuffer() {
        guard !outboundBuffer.isEmpty else { return }
        let buffered = outboundBuffer
        outboundBuffer.removeAll()
        for frame in buffered { send(frame) }
    }

    private func appendLog(_ msg: String) {
        let entry = "[\(shortTime())] \(msg)"
        log.append(entry)
        if log.count > 300 { log.removeFirst(log.count - 300) }
        print("[BleManager] \(entry)")
    }

    private func shortTime() -> String {
        let f = DateFormatter(); f.dateFormat = "HH:mm:ss"
        return f.string(from: .now)
    }
}

// MARK: - CBCentralManagerDelegate

extension BleManager: CBCentralManagerDelegate {

    nonisolated func centralManagerDidUpdateState(_ central: CBCentralManager) {
        Task { @MainActor in
            if central.state == .poweredOn {
                startScanning()
            }
        }
    }

    nonisolated func centralManager(_ central: CBCentralManager,
                                    willRestoreState dict: [String: Any]) {
        // State restoration: re-subscribe to discovered services
        let restored = dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral] ?? []
        Task { @MainActor in
            for p in restored {
                p.delegate = self
                peers[p.identifier] = Peer(peripheral: p)
            }
        }
    }

    nonisolated func centralManager(_ central: CBCentralManager,
                                    didDiscover peripheral: CBPeripheral,
                                    advertisementData: [String: Any],
                                    rssi RSSI: NSNumber) {
        Task { @MainActor in connectPeer(peripheral) }
    }

    nonisolated func centralManager(_ central: CBCentralManager,
                                    didConnect peripheral: CBPeripheral) {
        Task { @MainActor in
            peripheral.delegate = self
            peers[peripheral.identifier] = Peer(peripheral: peripheral)
            connectedPeers = peers.keys.map { $0.uuidString }
            appendLog("Connected ← \(peripheral.identifier.uuidString.prefix(8))")
            peripheral.discoverServices([BLE.serviceUUID])
            delegate?.bleManagerDidConnect(self)
            flushOutboundBuffer()
            scheduleIdleDisconnect(for: peripheral.identifier)
        }
    }

    nonisolated func centralManager(_ central: CBCentralManager,
                                    didFailToConnect peripheral: CBPeripheral,
                                    error: Error?) {
        Task { @MainActor in
            appendLog("Connection failed: \(error?.localizedDescription ?? "unknown")")
        }
    }

    nonisolated func centralManager(_ central: CBCentralManager,
                                    didDisconnectPeripheral peripheral: CBPeripheral,
                                    error: Error?) {
        Task { @MainActor in
            peers.removeValue(forKey: peripheral.identifier)
            idleTimers[peripheral.identifier]?.cancel()
            idleTimers.removeValue(forKey: peripheral.identifier)
            peerCooldown[peripheral.identifier] = Date().addingTimeInterval(Double(BLE.peerCooldownMs) / 1_000)
            connectedPeers = peers.keys.map { $0.uuidString }
            appendLog("Disconnected ↔ \(peripheral.identifier.uuidString.prefix(8))")
            delegate?.bleManagerDidDisconnect(self)
            // Resume scanning for other peers
            startScanning()
        }
    }
}

// MARK: - CBPeripheralDelegate (central discovers remote services)

extension BleManager: CBPeripheralDelegate {

    nonisolated func peripheral(_ peripheral: CBPeripheral,
                                didDiscoverServices error: Error?) {
        guard let services = peripheral.services else { return }
        for svc in services where svc.uuid == BLE.serviceUUID {
            peripheral.discoverCharacteristics([BLE.txCharUUID, BLE.rxCharUUID], for: svc)
        }
    }

    nonisolated func peripheral(_ peripheral: CBPeripheral,
                                didDiscoverCharacteristicsFor service: CBService,
                                error: Error?) {
        Task { @MainActor in
            for char in service.characteristics ?? [] {
                if char.uuid == BLE.txCharUUID {
                    peers[peripheral.identifier]?.txChar = char
                }
                if char.uuid == BLE.rxCharUUID {
                    peripheral.setNotifyValue(true, for: char)
                }
            }
        }
    }

    nonisolated func peripheral(_ peripheral: CBPeripheral,
                                didUpdateValueFor characteristic: CBCharacteristic,
                                error: Error?) {
        guard characteristic.uuid == BLE.rxCharUUID,
              let data = characteristic.value else { return }
        Task { @MainActor in
            refreshLastFrame(for: peripheral.identifier)
            delegate?.bleManager(self, didReceiveFrame: data)
        }
    }

    nonisolated func peripheral(_ peripheral: CBPeripheral,
                                didWriteValueFor characteristic: CBCharacteristic,
                                error: Error?) {
        if let error {
            Task { @MainActor in appendLog("Write error: \(error.localizedDescription)") }
        }
    }
}

// MARK: - CBPeripheralManagerDelegate

extension BleManager: CBPeripheralManagerDelegate {

    nonisolated func peripheralManagerDidUpdateState(_ peripheral: CBPeripheralManager) {
        Task { @MainActor in
            if peripheral.state == .poweredOn {
                setupGattServer()
            }
        }
    }

    nonisolated func peripheralManager(_ peripheral: CBPeripheralManager,
                                       willRestoreState dict: [String: Any]) {
        // Nothing extra needed — services will be re-added in didUpdateState
    }

    nonisolated func peripheralManager(_ peripheral: CBPeripheralManager,
                                       didAdd service: CBService, error: Error?) {
        Task { @MainActor in
            if let error {
                appendLog("Add service error: \(error.localizedDescription)")
            } else {
                startAdvertising()
            }
        }
    }

    nonisolated func peripheralManagerDidStartAdvertising(_ peripheral: CBPeripheralManager,
                                                          error: Error?) {
        Task { @MainActor in
            if let error {
                appendLog("Advertising error: \(error.localizedDescription)")
            } else {
                isAdvertising = true
                appendLog("Advertising started")
            }
        }
    }

    nonisolated func peripheralManager(_ peripheral: CBPeripheralManager,
                                       central: CBCentral,
                                       didSubscribeTo characteristic: CBCharacteristic) {
        Task { @MainActor in
            subscribedCentrals.append(central)
            appendLog("Central subscribed: \(central.identifier.uuidString.prefix(8))")
            delegate?.bleManagerDidConnect(self)
            flushOutboundBuffer()
        }
    }

    nonisolated func peripheralManager(_ peripheral: CBPeripheralManager,
                                       central: CBCentral,
                                       didUnsubscribeFrom characteristic: CBCharacteristic) {
        Task { @MainActor in
            subscribedCentrals.removeAll { $0.identifier == central.identifier }
            appendLog("Central unsubscribed: \(central.identifier.uuidString.prefix(8))")
            delegate?.bleManagerDidDisconnect(self)
        }
    }

    nonisolated func peripheralManager(_ peripheral: CBPeripheralManager,
                                       didReceiveWrite requests: [CBATTRequest]) {
        for req in requests {
            guard req.characteristic.uuid == BLE.txCharUUID,
                  let data = req.value else { continue }
            peripheral.respond(to: req, withResult: .success)
            Task { @MainActor in
                delegate?.bleManager(self, didReceiveFrame: data)
            }
        }
    }

    // MARK: Private — GATT server setup

    private func setupGattServer() {
        let rxCharacteristic = CBMutableCharacteristic(
            type: BLE.rxCharUUID,
            properties: [.notify],
            value: nil,
            permissions: []
        )
        let txCharacteristic = CBMutableCharacteristic(
            type: BLE.txCharUUID,
            properties: [.write, .writeWithoutResponse],
            value: nil,
            permissions: [.writeable]
        )
        let service = CBMutableService(type: BLE.serviceUUID, primary: true)
        service.characteristics = [rxCharacteristic, txCharacteristic]
        rxChar = rxCharacteristic
        peripheral.add(service)
    }
}
