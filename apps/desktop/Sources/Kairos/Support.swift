import AppKit
import ApplicationServices
import AVFoundation
import Carbon.HIToolbox
import KairosCore
import Security
import ServiceManagement
import SwiftUI

enum Keychain {
    static let service = "com.kairos.desktop"

    static func get(_ account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
            kSecAttrAccount as String: account, kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var out: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    @discardableResult
    static func set(_ value: String, for account: String) -> Bool {
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account,
        ]
        SecItemDelete(base as CFDictionary)
        guard !value.isEmpty else { return true }
        var add = base
        add[kSecValueData as String] = Data(value.utf8)
        return SecItemAdd(add as CFDictionary, nil) == errSecSuccess
    }
}

extension Color {
    init(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        let v = UInt64(s, radix: 16) ?? 0x4F7CFF
        self.init(red: Double((v >> 16) & 0xFF) / 255, green: Double((v >> 8) & 0xFF) / 255, blue: Double(v & 0xFF) / 255)
    }
}

enum GroupPalette {
    static let colors = ["#4F7CFF", "#2FA36B", "#E0703A", "#B45BD6", "#D6455B", "#2AA7B8", "#C9A227", "#6B7280"]
    static let icons = ["briefcase.fill", "book.fill", "house.fill", "paintbrush.fill", "hammer.fill", "gamecontroller.fill",
                        "music.note", "graduationcap.fill", "chevron.left.forwardslash.chevron.right", "pencil", "heart.fill", "square.grid.2x2"]
}

/// An installed application, found by scanning the usual folders.
struct InstalledApp: Identifiable, Hashable {
    var id: String { bundleId }
    let bundleId: String
    let name: String
    let url: URL

    static func scan() -> [InstalledApp] {
        let fm = FileManager.default
        let roots = ["/Applications", "/System/Applications", "/System/Applications/Utilities", "/Applications/Utilities",
                     NSHomeDirectory() + "/Applications"]
        var seen = Set<String>(), out: [InstalledApp] = []
        for root in roots {
            for item in (try? fm.contentsOfDirectory(atPath: root)) ?? [] where item.hasSuffix(".app") {
                let url = URL(fileURLWithPath: root).appendingPathComponent(item)
                guard let id = Bundle(url: url)?.bundleIdentifier, seen.insert(id).inserted else { continue }
                out.append(.init(bundleId: id, name: fm.displayName(atPath: url.path).replacingOccurrences(of: ".app", with: ""), url: url))
            }
        }
        return out.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }
}

enum Apps {
    static func url(_ bundleId: String) -> URL? { NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleId) }

    // Kairos Bar redraws on every pointer move, so names and icons are looked up once.
    private static var names: [String: String] = [:]
    private static var icons: [String: NSImage] = [:]

    static func name(_ bundleId: String) -> String {
        if let n = names[bundleId] { return n }
        guard let u = url(bundleId) else { return bundleId }
        let n = FileManager.default.displayName(atPath: u.path).replacingOccurrences(of: ".app", with: "")
        names[bundleId] = n
        return n
    }

    static func icon(_ bundleId: String) -> NSImage {
        if let i = icons[bundleId] { return i }
        guard let u = url(bundleId) else {
            return NSImage(systemSymbolName: "questionmark.app.dashed", accessibilityDescription: nil) ?? NSImage()
        }
        let i = NSWorkspace.shared.icon(forFile: u.path)
        icons[bundleId] = i
        return i
    }

    static func running(_ bundleId: String) -> NSRunningApplication? {
        NSWorkspace.shared.runningApplications.first { $0.bundleIdentifier == bundleId }
    }

    static func open(_ bundleId: String) {
        if let running = NSWorkspace.shared.runningApplications.first(where: { $0.bundleIdentifier == bundleId }) {
            running.unhide()
            running.activate()
        } else if let u = url(bundleId) {
            NSWorkspace.shared.openApplication(at: u, configuration: .init())
        }
    }

    /// The display showing the app's frontmost normal window. Window bounds need no permission.
    static func displayShowing(pid: pid_t) -> CGDirectDisplayID? {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]],
              let info = list.first(where: { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }),
              let dict = info[kCGWindowBounds as String] as CFTypeRef?,
              let bounds = CGRect(dictionaryRepresentation: dict as! CFDictionary) // swiftlint:disable:this force_cast
        else { return nil }
        var ids = [CGDirectDisplayID](repeating: 0, count: 16), count: UInt32 = 0
        guard CGGetActiveDisplayList(16, &ids, &count) == .success else { return nil }
        return Screens.display(showing: bounds, among: ids.prefix(Int(count)).map { ($0, CGDisplayBounds($0)) })
    }

    /// Title of the frontmost window. Needs the Accessibility permission; nil without it.
    static func frontWindowTitle(pid: pid_t) -> String? {
        guard AXIsProcessTrusted() else { return nil }
        let app = AXUIElementCreateApplication(pid)
        var window: CFTypeRef?
        guard AXUIElementCopyAttributeValue(app, kAXFocusedWindowAttribute as CFString, &window) == .success, let window else { return nil }
        var title: CFTypeRef?
        // swiftlint:disable:next force_cast
        guard AXUIElementCopyAttributeValue(window as! AXUIElement, kAXTitleAttribute as CFString, &title) == .success else { return nil }
        return title as? String
    }
}

enum BridgeInstaller {
    /// Path of the native host inside the running app bundle.
    static var hostPath: String {
        Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/kairos-host").path
    }

    static var extensionsFolder: URL? {
        let u = Bundle.main.bundleURL.appendingPathComponent("Contents/Resources/extensions")
        return FileManager.default.fileExists(atPath: u.path) ? u : nil
    }

    /// Writes the native messaging manifest for every Chromium browser that has a profile
    /// folder. Returns the browsers it was installed for.
    @discardableResult
    static func install() -> [String] {
        let fm = FileManager.default
        let support = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        guard fm.isExecutableFile(atPath: hostPath),
              let json = try? JSONSerialization.data(withJSONObject: NativeFraming.hostManifest(hostPath: hostPath), options: [.prettyPrinted, .sortedKeys])
        else { return [] }
        var done: [String] = []
        for browser in NativeFraming.browserDirs {
            let dir = support.appendingPathComponent(browser.dir)
            // Only browsers that are installed have the parent profile folder.
            guard fm.fileExists(atPath: dir.deletingLastPathComponent().path) else { continue }
            do {
                try fm.createDirectory(at: dir, withIntermediateDirectories: true)
                try json.write(to: dir.appendingPathComponent("\(BridgePaths.hostName).json"), options: .atomic)
                done.append(browser.name)
            } catch { continue }
        }
        return done
    }

    static func installedBrowsers() -> [String] {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return NativeFraming.browserDirs.compactMap { b in
            let file = support.appendingPathComponent(b.dir).appendingPathComponent("\(BridgePaths.hostName).json")
            guard let data = try? Data(contentsOf: file),
                  let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  obj["path"] as? String == hostPath else { return nil }
            return b.name
        }
    }
}

enum LoginItem {
    static var enabled: Bool { SMAppService.mainApp.status == .enabled }
    static func set(_ on: Bool) {
        do { if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() } } catch {
            NSLog("Kairos: login item change failed: \(error.localizedDescription)")
        }
    }
}

func clock(_ seconds: TimeInterval) -> String {
    let s = max(0, Int(seconds.rounded(.up)))
    return String(format: "%d:%02d", s / 60, s % 60)
}

enum Spaces {
    private static let digits = [kVK_ANSI_1, kVK_ANSI_2, kVK_ANSI_3, kVK_ANSI_4, kVK_ANSI_5, kVK_ANSI_6, kVK_ANSI_7, kVK_ANSI_8, kVK_ANSI_9]

    /// macOS has no public API for Spaces, so this presses Control-number, which works when
    /// "Switch to Desktop N" is enabled in System Settings > Keyboard > Keyboard Shortcuts >
    /// Mission Control. Needs the Accessibility permission. Returns false when it could not send.
    @discardableResult
    static func switchTo(_ number: Int) -> Bool {
        guard (1...9).contains(number), AXIsProcessTrusted() else { return false }
        let source = CGEventSource(stateID: .hidSystemState)
        let code = CGKeyCode(digits[number - 1])
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else { return false }
        down.flags = .maskControl
        up.flags = .maskControl
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
        return true
    }
}

/// Plays cloud-voice audio for Kairos Read. One clip at a time.
@MainActor
final class SpeechPlayer: NSObject, AVAudioPlayerDelegate {
    private var player: AVAudioPlayer?
    private var onDone: ((_ stopped: Bool) -> Void)?

    func play(_ data: Data, rate: Float, done: @escaping (_ stopped: Bool) -> Void) throws {
        stop()
        let p = try AVAudioPlayer(data: data)
        p.enableRate = true
        p.rate = min(2, max(0.5, rate))
        p.delegate = self
        player = p
        onDone = done
        p.play()
    }

    func stop() {
        player?.stop()
        player = nil
        let done = onDone
        onDone = nil
        done?(true)
    }

    func pause() { player?.pause() }
    func resume() { player?.play() }

    nonisolated func audioPlayerDidFinishPlaying(_ finished: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor in
            guard self.player === finished else { return }
            self.player = nil
            let done = self.onDone
            self.onDone = nil
            done?(false)
        }
    }
}
