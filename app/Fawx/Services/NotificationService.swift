import Foundation
import UserNotifications

#if os(macOS)
import AppKit
#elseif canImport(UIKit)
import UIKit
#endif

@MainActor
final class NotificationService {
    static let shared = NotificationService()

    private init() {}

    func requestPermission() async -> Bool {
        let center = UNUserNotificationCenter.current()
        let settings = await center.notificationSettings()

        switch settings.authorizationStatus {
        case .authorized, .provisional, .ephemeral:
            return true
        case .notDetermined:
            do {
                return try await center.requestAuthorization(options: [.alert, .sound, .badge])
            } catch {
                return false
            }
        case .denied:
            return false
        @unknown default:
            return false
        }
    }

    func send(title: String = "Fawx", body: String) async {
        let trimmedBody = body.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedBody.isEmpty else {
            return
        }

        guard await requestPermission(), shouldPresentNotification else {
            return
        }

        let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let content = UNMutableNotificationContent()
        content.title = trimmedTitle.isEmpty ? "Fawx" : trimmedTitle
        content.body = trimmedBody
        content.sound = .default

        let request = UNNotificationRequest(
            identifier: UUID().uuidString,
            content: content,
            trigger: nil
        )

        do {
            try await UNUserNotificationCenter.current().add(request)
        } catch {
            print("NotificationService: failed to deliver notification: \(error)")
        }
    }

    private var shouldPresentNotification: Bool {
        #if os(macOS)
        !NSApp.isActive
        #elseif canImport(UIKit)
        UIApplication.shared.applicationState != .active
        #else
        true
        #endif
    }
}
