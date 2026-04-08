import Foundation

extension String {
    /// Returns `nil` for empty strings so optional UI values can stay typed as
    /// "missing" instead of carrying empty-string sentinels through the app.
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
