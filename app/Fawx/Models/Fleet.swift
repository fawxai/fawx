import Foundation

struct FleetOverviewResponse: Codable, Sendable, Hashable {
    let totalNodes: Int
    let healthyNodes: Int
    let degradedNodes: Int
    let offlineNodes: Int
    let activeTasks: Int
    let queuedTasks: Int
    let updatedAt: Int

    enum CodingKeys: String, CodingKey {
        case totalNodes = "total_nodes"
        case healthyNodes = "healthy_nodes"
        case degradedNodes = "degraded_nodes"
        case offlineNodes = "offline_nodes"
        case activeTasks = "active_tasks"
        case queuedTasks = "queued_tasks"
        case updatedAt = "updated_at"
    }
}

struct FleetNodesResponse: Codable, Sendable, Hashable {
    let nodes: [FleetNodeSummary]
    let total: Int
}

struct FleetNodeSummary: Codable, Sendable, Hashable, Identifiable {
    let id: String
    let name: String
    let status: FleetNodeHealth
    let lastSeenAt: Int
    let activeTasks: Int
    let capabilities: [String]

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case status
        case lastSeenAt = "last_seen_at"
        case activeTasks = "active_tasks"
        case capabilities
    }

    var displayStatus: FleetNodeDisplayStatus {
        FleetNodeDisplayStatus(health: status, activeTasks: activeTasks)
    }
}

struct FleetNodeDetailResponse: Codable, Sendable, Hashable, Identifiable {
    let id: String
    let name: String
    let status: FleetNodeHealth
    let lastSeenAt: Int
    let activeTasks: Int
    let queuedTasks: Int
    let capabilities: [String]
    let endpoint: String
    let registeredAt: Int

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case status
        case lastSeenAt = "last_seen_at"
        case activeTasks = "active_tasks"
        case queuedTasks = "queued_tasks"
        case capabilities
        case endpoint
        case registeredAt = "registered_at"
    }

    var displayStatus: FleetNodeDisplayStatus {
        FleetNodeDisplayStatus(health: status, activeTasks: activeTasks)
    }
}

struct FleetDispatchTaskResponse: Codable, Sendable, Hashable {
    let accepted: Bool
    let nodeID: String
    let taskID: String
    let status: String

    enum CodingKeys: String, CodingKey {
        case accepted
        case nodeID = "node_id"
        case taskID = "task_id"
        case status
    }
}

enum FleetNodeHealth: String, Codable, Sendable, Hashable {
    case healthy
    case degraded
    case offline
}

enum FleetNodeDisplayStatus: Sendable, Hashable {
    case online
    case busy
    case stale
    case offline

    init(health: FleetNodeHealth, activeTasks: Int) {
        switch health {
        case .healthy:
            self = activeTasks > 0 ? .busy : .online
        case .degraded:
            self = .stale
        case .offline:
            self = .offline
        }
    }

    var title: String {
        switch self {
        case .online:
            "Online"
        case .busy:
            "Busy"
        case .stale:
            "Stale"
        case .offline:
            "Offline"
        }
    }

    var priority: Int {
        switch self {
        case .busy:
            0
        case .online:
            1
        case .stale:
            2
        case .offline:
            3
        }
    }
}
