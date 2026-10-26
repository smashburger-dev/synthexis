import CoreBluetooth
import Foundation

// heart-rate-ble probe (P5): scan for BLE peripherals advertising Heart Rate
// Service 0x180D, also log any peripheral named *Beats*/*Powerbeats*, connect,
// subscribe to 0x2A37, parse HR measurement, log bpm for 60 s.

let runSeconds: Double = 60

final class Logger {
    let handle: FileHandle?
    init() {
        let url = URL(fileURLWithPath: CommandLine.arguments[0])
            .deletingLastPathComponent()
            .appendingPathComponent("raw_hr.log")
        FileManager.default.createFile(atPath: url.path, contents: nil)
        handle = try? FileHandle(forWritingTo: url)
        print("log file: \(url.path)")
    }
    func log(_ s: String) {
        let line = String(format: "[%@] %@",
                          ISO8601DateFormatter().string(from: Date()), s)
        print(line)
        if let h = handle { h.write((line + "\n").data(using: .utf8)!) }
    }
}

let log = Logger()
let hrService = CBUUID(string: "180D")
let hrMeasurement = CBUUID(string: "2A37")

final class Central: NSObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    var central: CBCentralManager!
    var target: CBPeripheral?
    var seen = Set<UUID>()

    override init() {
        super.init()
        central = CBCentralManager(delegate: self, queue: nil)
    }

    func centralManagerDidUpdateState(_ c: CBCentralManager) {
        log.log("central state = \(c.state.rawValue)")
        if c.state == .poweredOn {
            // Unfiltered scan: catches 0x180D advertisers AND any Beats
            // peripheral regardless of what it advertises.
            c.scanForPeripherals(withServices: nil,
                                 options: [CBCentralManagerScanOptionAllowDuplicatesKey: false])
            log.log("scanning (unfiltered) for \(runSeconds)s ...")
        }
    }

    func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral,
                        advertisementData: [String: Any], rssi: NSNumber) {
        guard !seen.contains(p.identifier) else { return }
        let name = p.name ?? advertisementData[CBAdvertisementDataLocalNameKey] as? String ?? ""
        let services = (advertisementData[CBAdvertisementDataServiceUUIDsKey] as? [CBUUID]) ?? []
        let isHR = services.contains(hrService)
        let isBeats = name.localizedCaseInsensitiveContains("beats")
        if !isHR && !isBeats { return }
        seen.insert(p.identifier)
        log.log("discovered name=\"\(name)\" rssi=\(rssi) services=\(services) hrAd=\(isHR)")
        if isHR && target == nil {
            target = p
            c.stopScan()
            log.log("connecting to \(name.isEmpty ? p.identifier.uuidString : name) ...")
            c.connect(p, options: nil)
        }
    }

    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) {
        log.log("connected, discovering services")
        p.delegate = self
        p.discoverServices([hrService])
    }

    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) {
        log.log("connect failed: \(error?.localizedDescription ?? "?")")
    }

    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        guard let s = p.services?.first(where: { $0.uuid == hrService }) else {
            log.log("no 180D service on peripheral")
            return
        }
        p.discoverCharacteristics([hrMeasurement], for: s)
    }

    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor s: CBService, error: Error?) {
        guard let ch = s.characteristics?.first(where: { $0.uuid == hrMeasurement }) else {
            log.log("no 2A37 characteristic")
            return
        }
        p.setNotifyValue(true, for: ch)
        log.log("subscribed to 2A37")
    }

    func peripheral(_ p: CBPeripheral, didUpdateValueFor ch: CBCharacteristic, error: Error?) {
        guard ch.uuid == hrMeasurement, let d = ch.value, d.count >= 2 else { return }
        let flags = d[0]
        let bpm: Int = (flags & 1) == 0 ? Int(d[1]) : Int(d[1]) | (Int(d[2]) << 8)
        log.log("HR \(bpm) bpm (flags=0x\(String(flags, radix: 16)))")
    }
}

let central = Central()
RunLoop.main.run(until: Date().addingTimeInterval(runSeconds))
log.log("DONE seen=\(central.seen.count) targetConnected=\(central.target != nil)")
