import Foundation

extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
