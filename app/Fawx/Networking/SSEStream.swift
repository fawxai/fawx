import Foundation

enum SSEEvent: Sendable, Hashable {
    case textDelta(String)
    case toolCallStart(id: String?, name: String?)
    case toolCallDelta(id: String?, argumentsDelta: String)
    case toolCallComplete(id: String?, name: String?, arguments: String)
    case toolResult(id: String?, output: String, isError: Bool)
    case phase(String)
    case engineError(category: String, message: String, recoverable: Bool)
    case done(response: String?)
    case error(String)
}

struct SSEParser {
    private var currentEventName = ""
    private var currentDataLines: [String] = []

    mutating func parseLine(_ line: String) throws -> [SSEEvent] {
        if line.hasPrefix(":") {
            return []
        }

        if line.isEmpty {
            return try flush()
        }

        if line.hasPrefix("event:") {
            currentEventName = line
                .dropFirst("event:".count)
                .trimmingCharacters(in: .whitespaces)
            return []
        }

        if line.hasPrefix("data:") {
            let data = line.dropFirst("data:".count)
            currentDataLines.append(String(data).trimmingPrefixSpace())
            return []
        }

        return []
    }

    mutating func finish() throws -> [SSEEvent] {
        try flush()
    }

    private mutating func flush() throws -> [SSEEvent] {
        let eventName = currentEventName.isEmpty ? "message" : currentEventName
        let data = currentDataLines.joined(separator: "\n")
        currentEventName = ""
        currentDataLines.removeAll(keepingCapacity: true)

        guard !data.isEmpty else {
            return []
        }

        guard let event = try Self.decode(eventName: eventName, data: data) else {
            return []
        }
        return [event]
    }

    private static func decode(eventName: String, data: String) throws -> SSEEvent? {
        let decoder = JSONDecoder()

        switch eventName {
        case "text_delta":
            let payload = try decoder.decode(TextDeltaPayload.self, from: Data(data.utf8))
            return .textDelta(payload.text)
        case "tool_call_start":
            let payload = try decoder.decode(ToolCallStartPayload.self, from: Data(data.utf8))
            return .toolCallStart(id: payload.id, name: payload.name)
        case "tool_call_delta":
            let payload = try decoder.decode(ToolCallDeltaPayload.self, from: Data(data.utf8))
            return .toolCallDelta(
                id: payload.id,
                argumentsDelta: payload.argumentsDelta
            )
        case "tool_call_complete":
            let payload = try decoder.decode(ToolCallCompletePayload.self, from: Data(data.utf8))
            return .toolCallComplete(
                id: payload.id,
                name: payload.name,
                arguments: payload.arguments ?? ""
            )
        case "tool_result":
            let payload = try decoder.decode(ToolResultPayload.self, from: Data(data.utf8))
            return .toolResult(
                id: payload.id,
                output: payload.output ?? "",
                isError: payload.isError
            )
        case "phase":
            let payload = try decoder.decode(PhasePayload.self, from: Data(data.utf8))
            return .phase(payload.phase)
        case "engine_error":
            let payload = try decoder.decode(EngineErrorPayload.self, from: Data(data.utf8))
            return .engineError(
                category: payload.category,
                message: payload.message,
                recoverable: payload.recoverable
            )
        case "done":
            let payload = try decoder.decode(DonePayload.self, from: Data(data.utf8))
            return .done(response: payload.response)
        case "error":
            let payload = try decoder.decode(FatalErrorPayload.self, from: Data(data.utf8))
            return .error(payload.error)
        default:
            return nil
        }
    }
}

private struct TextDeltaPayload: Decodable {
    let text: String
}

private struct ToolCallStartPayload: Decodable {
    let id: String?
    let name: String?
}

private struct ToolCallCompletePayload: Decodable {
    let id: String?
    let name: String?
    let arguments: String?
}

private struct ToolCallDeltaPayload: Decodable {
    let id: String?
    let argumentsDelta: String

    enum CodingKeys: String, CodingKey {
        case id
        case argumentsDelta = "args_delta"
    }
}

private struct ToolResultPayload: Decodable {
    let id: String?
    let output: String?
    let isError: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case output
        case isError = "is_error"
    }
}

private struct PhasePayload: Decodable {
    let phase: String
}

private struct EngineErrorPayload: Decodable {
    let category: String
    let message: String
    let recoverable: Bool
}

private struct DonePayload: Decodable {
    let response: String?
}

private struct FatalErrorPayload: Decodable {
    let error: String
}

private extension String {
    func trimmingPrefixSpace() -> String {
        hasPrefix(" ") ? String(dropFirst()) : self
    }
}
