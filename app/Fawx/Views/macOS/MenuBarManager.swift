#if os(macOS)
import AppKit
import SwiftUI

@MainActor
final class MenuBarManager: NSObject {
    private weak var appState: AppState?
    private let statusItem: NSStatusItem
    private var pollTimer: Timer?

    init(appState: AppState) {
        self.appState = appState
        self.statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        super.init()
        configureStatusItem()
        rebuildMenu()
        startPolling()
    }

    func updateAppState(_ appState: AppState) {
        self.appState = appState
        refreshStatusDisplay()
        rebuildMenu()
    }

    private func configureStatusItem() {
        if let button = statusItem.button {
            button.imagePosition = .noImage
            button.toolTip = "Fawx"
        }
        refreshStatusDisplay()
    }

    private func startPolling() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.pollServerStatus()
            }
        }
    }

    private func pollServerStatus() {
        guard let appState else {
            return
        }

        Task { @MainActor [weak self] in
            if appState.isConfigured {
                await appState.refreshPhase4State()
            }
            self?.refreshStatusDisplay()
            self?.rebuildMenu()
        }
    }

    private func refreshStatusDisplay() {
        guard let button = statusItem.button else {
            return
        }

        let snapshot = snapshot()
        button.image = nil
        button.attributedTitle = statusItemTitle(snapshot: snapshot)
        button.toolTip = "\(snapshot.title)\n\(snapshot.detail)"
    }

    private func rebuildMenu() {
        let menu = NSMenu()

        let headerItem = NSMenuItem()
        headerItem.isEnabled = false
        headerItem.view = NSHostingView(rootView: MenuBarView(snapshot: snapshot()))
        menu.addItem(headerItem)
        menu.addItem(.separator())

        menu.addItem(makeItem(title: "Open Fawx", action: #selector(openFawx)))
        menu.addItem(makeItem(title: "Restart Server", action: #selector(restartServer)))
        menu.addItem(makeItem(title: "Stop Server", action: #selector(stopServer)))
        menu.addItem(makeItem(title: "Stop Server & Quit", action: #selector(stopServerAndQuit)))
        menu.addItem(.separator())
        menu.addItem(makeItem(title: "Quit", action: #selector(quitApp)))

        statusItem.menu = menu
    }

    private func makeItem(title: String, action: Selector) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        return item
    }

    private func snapshot() -> MenuBarStatusSnapshot {
        guard let appState else {
            return MenuBarStatusSnapshot(
                title: "Fawx",
                detail: "Status unavailable",
                color: .fawxTextSecondary
            )
        }

        if !appState.isConfigured {
            return MenuBarStatusSnapshot(
                title: "Fawx setup required",
                detail: "Open the app to finish setup.",
                color: .fawxWarning
            )
        }

        let status = appState.localServerStatus?.status.lowercased() ?? fallbackStatus(for: appState.connectionStatus)
        let detail = "\(appState.displayedHost)\(appState.displayedPort.map { ":\($0)" } ?? "")"

        switch status {
        case "running", "connected":
            return MenuBarStatusSnapshot(
                title: "Fawx is running",
                detail: detail,
                color: .fawxSuccess
            )
        case "starting", "connecting", "reconnecting":
            return MenuBarStatusSnapshot(
                title: "Fawx is reconnecting",
                detail: detail,
                color: .fawxWarning
            )
        case "stopped", "disconnected":
            return MenuBarStatusSnapshot(
                title: "Fawx is stopped",
                detail: detail,
                color: .fawxError
            )
        default:
            return MenuBarStatusSnapshot(
                title: "Fawx status unknown",
                detail: detail,
                color: .fawxTextSecondary
            )
        }
    }

    private func fallbackStatus(for connectionStatus: ConnectionStatus) -> String {
        switch connectionStatus {
        case .connected:
            "running"
        case .connecting, .reconnecting:
            "starting"
        case .disconnected:
            "stopped"
        }
    }

    private func nsColor(from color: Color) -> NSColor {
        NSColor(color)
    }

    private func statusItemTitle(snapshot: MenuBarStatusSnapshot) -> NSAttributedString {
        let title = NSMutableAttributedString(
            string: "🦊 ",
            attributes: [
                .font: NSFont.systemFont(ofSize: 14),
            ]
        )

        title.append(
            NSAttributedString(
                string: "●",
                attributes: [
                    .font: NSFont.systemFont(ofSize: 12, weight: .bold),
                    .foregroundColor: nsColor(from: snapshot.color),
                ]
            )
        )

        return title
    }

    private func isPrimaryAppWindow(_ window: NSWindow) -> Bool {
        let className = NSStringFromClass(type(of: window))
        guard !className.contains("NSStatusBarWindow") else {
            return false
        }
        return window.canBecomeKey || window.canBecomeMain
    }

    @objc
    private func openFawx() {
        NSApp.unhide(nil)
        NSApp.activate(ignoringOtherApps: true)

        DispatchQueue.main.async { [weak self] in
            guard let self else {
                return
            }

            let window = NSApp.orderedWindows.first(where: self.isPrimaryAppWindow)
                ?? NSApp.windows.first(where: self.isPrimaryAppWindow)

            window?.deminiaturize(nil)
            window?.orderFrontRegardless()
            window?.makeKeyAndOrderFront(nil)
        }
    }

    @objc
    private func restartServer() {
        guard let appState else {
            return
        }

        Task { @MainActor in
            await performServerAction {
                _ = try await appState.restartLocalServer()
            }
        }
    }

    @objc
    private func stopServer() {
        guard let appState else {
            return
        }

        Task { @MainActor in
            await performServerAction {
                _ = try await appState.stopLocalServer()
            }
        }
    }

    @objc
    private func stopServerAndQuit() {
        guard let appState else {
            quitApp()
            return
        }

        Task { @MainActor in
            do {
                _ = try await appState.stopLocalServer()
            } catch {
                appState.showToast(message: error.localizedDescription, style: .error)
            }
            quitApp()
        }
    }

    @objc
    private func quitApp() {
        NSApp.terminate(nil)
    }

    private func performServerAction(_ action: @escaping @MainActor () async throws -> Void) async {
        guard let appState else {
            return
        }

        do {
            try await action()
        } catch {
            appState.showToast(message: error.localizedDescription, style: .error)
        }

        await appState.refreshPhase4State()
        refreshStatusDisplay()
        rebuildMenu()
    }
}
#endif
