# Fawx Swift App

`project.yml` is the canonical build definition for the native app targets. Regenerate `Fawx.xcodeproj` with `xcodegen` after changing target, scheme, or dependency wiring.

`Package.swift` is retained only as a lightweight package view of the shared Swift sources for package-aware tooling. The application itself should be built and tested through the XcodeGen project, not through SwiftPM.
