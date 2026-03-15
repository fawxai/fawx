import Foundation
import Security

enum KeychainHelper {
    static let defaultService = "ai.fawx.app"

    static func token(forServer account: String, service: String = defaultService) throws -> String? {
        let query = tokenQuery(forServer: account, service: service, includeData: true)

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            guard
                let data = item as? Data,
                let token = String(data: data, encoding: .utf8)
            else {
                return nil
            }
            return token
        case errSecItemNotFound:
            return nil
        default:
            throw KeychainError.operationFailed(status)
        }
    }

    static func saveToken(_ token: String, forServer account: String, service: String = defaultService) throws {
        let data = Data(token.utf8)
        let query = tokenQuery(forServer: account, service: service)

        let attributes: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlocked,
        ]

        let existingStatus = SecItemCopyMatching(query as CFDictionary, nil)
        switch existingStatus {
        case errSecSuccess:
            let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
            guard status == errSecSuccess else {
                throw KeychainError.operationFailed(status)
            }
        case errSecItemNotFound:
            var newItem = query
            attributes.forEach { newItem[$0.key] = $0.value }
            let status = SecItemAdd(newItem as CFDictionary, nil)
            guard status == errSecSuccess else {
                throw KeychainError.operationFailed(status)
            }
        default:
            throw KeychainError.operationFailed(existingStatus)
        }
    }

    static func deleteToken(forServer account: String, service: String = defaultService) throws {
        let query = tokenQuery(forServer: account, service: service)

        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainError.operationFailed(status)
        }
    }

    private static func tokenQuery(
        forServer account: String,
        service: String,
        includeData: Bool = false
    ) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        if includeData {
            query[kSecReturnData as String] = true
        }
        return query
    }
}

enum KeychainError: LocalizedError, Sendable {
    case operationFailed(OSStatus)

    var errorDescription: String? {
        switch self {
        case .operationFailed(let status):
            if let message = SecCopyErrorMessageString(status, nil) as String? {
                return message
            }
            return "Keychain operation failed with status \(status)."
        }
    }
}
