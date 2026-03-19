import Foundation

struct ExperimentsListResponse: Codable, Sendable, Hashable {
    let experiments: [ExperimentSummary]
    let total: Int
}

struct ExperimentSummary: Codable, Sendable, Hashable, Identifiable {
    let id: String
    let name: String
    let kind: ExperimentKind
    let status: ExperimentStatus
    let scoreSummary: String
    let createdAt: Int

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case kind
        case status
        case scoreSummary = "score_summary"
        case createdAt = "created_at"
    }
}

struct ExperimentDetail: Codable, Sendable, Hashable, Identifiable {
    let id: String
    let name: String
    let kind: ExperimentKind
    let status: ExperimentStatus
    let config: ExperimentConfig
    let createdAt: Int
    let startedAt: Int?
    let completedAt: Int?
    let fleetNodes: [String]
    let progress: ExperimentProgress?
    let result: ExperimentRunResult?
    let error: String?

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case kind
        case status
        case config
        case createdAt = "created_at"
        case startedAt = "started_at"
        case completedAt = "completed_at"
        case fleetNodes = "fleet_nodes"
        case progress
        case result
        case error
    }
}

enum ExperimentKind: String, Codable, Sendable, Hashable {
    case proofOfFitness = "proof_of_fitness"
    case analysisOnly = "analysis_only"
    case tournament

    var displayName: String {
        switch self {
        case .proofOfFitness:
            "Proof of Fitness"
        case .analysisOnly:
            "Analysis Only"
        case .tournament:
            "Tournament"
        }
    }
}

enum ExperimentStatus: String, Codable, Sendable, Hashable {
    case queued
    case running
    case completed
    case stopped
    case failed

    var displayName: String {
        switch self {
        case .queued:
            "Queued"
        case .running:
            "Running"
        case .completed:
            "Completed"
        case .stopped:
            "Cancelled"
        case .failed:
            "Failed"
        }
    }
}

struct ExperimentConfig: Codable, Sendable, Hashable {
    let population: Int
    let rounds: Int
    let minConfidence: String?
    let outputMode: String?

    enum CodingKeys: String, CodingKey {
        case population
        case rounds
        case minConfidence = "min_confidence"
        case outputMode = "output_mode"
    }
}

struct ExperimentProgress: Codable, Sendable, Hashable {
    let completedMatches: Int
    let totalMatches: Int

    enum CodingKeys: String, CodingKey {
        case completedMatches = "completed_matches"
        case totalMatches = "total_matches"
    }
}

struct ExperimentRunResult: Codable, Sendable, Hashable {
    let plansGenerated: Int
    let proposalsWritten: [String]
    let branchesCreated: [String]
    let skipped: [SkippedItem]

    enum CodingKeys: String, CodingKey {
        case plansGenerated = "plans_generated"
        case proposalsWritten = "proposals_written"
        case branchesCreated = "branches_created"
        case skipped
    }
}

struct SkippedItem: Codable, Sendable, Hashable {
    let name: String
    let reason: String
}

struct ExperimentResultsResponse: Codable, Sendable, Hashable {
    let id: String
    let status: ExperimentStatus
    let leaders: [ExperimentLeader]
    let tournament: ExperimentTournament?
    let plansGenerated: Int
    let proposalsWritten: [String]
    let branchesCreated: [String]
    let skipped: [SkippedItem]

    enum CodingKeys: String, CodingKey {
        case id
        case status
        case leaders
        case tournament
        case plansGenerated = "plans_generated"
        case proposalsWritten = "proposals_written"
        case branchesCreated = "branches_created"
        case skipped
    }
}

struct ExperimentLeader: Codable, Sendable, Hashable, Identifiable {
    let chainID: String
    let name: String
    let score: Double
    let risk: String

    enum CodingKeys: String, CodingKey {
        case chainID = "chain_id"
        case name
        case score
        case risk
    }

    var id: String { chainID }
}

struct ExperimentTournament: Codable, Sendable, Hashable {
    let round: Int
    let totalRounds: Int
    let remainingMatches: Int

    enum CodingKeys: String, CodingKey {
        case round
        case totalRounds = "total_rounds"
        case remainingMatches = "remaining_matches"
    }
}

struct StopExperimentResponse: Codable, Sendable, Hashable {
    let id: String
    let stopping: Bool
}
