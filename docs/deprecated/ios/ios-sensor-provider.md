# iOS Sensor Provider — Permission Model & Protocol Skeleton

Date: 2026-02-24
Status: Draft v1
Tracks: Issue #775

## 1. Purpose

Define the iOS sensor access layer that maps Android's `AndroidSensorProvider` to
iOS-native frameworks (CoreLocation, CoreMotion, HealthKit) with a permission
model that respects iOS privacy requirements and integrates with the policy engine.

This document covers:

1. Android → iOS sensor capability mapping
2. iOS permission model and authorization lifecycle
3. `IOSSensorProvider` protocol skeleton and concrete providers
4. Integration with PolicyEngine risk tiers
5. Graceful degradation when permissions are denied

## 2. Android → iOS Sensor Mapping

| Android Sensor | iOS Framework | iOS API | Availability | Notes |
|---|---|---|---|---|
| `TYPE_ACCELEROMETER` | CoreMotion | `CMMotionManager.accelerometerData` | Always available | No permission required |
| `TYPE_GYROSCOPE` | CoreMotion | `CMMotionManager.gyroData` | Always available | No permission required |
| `TYPE_MAGNETIC_FIELD` | CoreMotion | `CMMotionManager.magnetometerData` | Always available | No permission required |
| `TYPE_PRESSURE` | CoreMotion | `CMAltimeter.relativeAltitude` | Always available | Barometric pressure via altimeter |
| `TYPE_LIGHT` | — | — | Not available | No public API for ambient light |
| `TYPE_PROXIMITY` | — | — | Not available | No public API for proximity |
| GPS / Fine Location | CoreLocation | `CLLocationManager` | Permission required | `kCLAuthorizationStatusAuthorizedWhenInUse` or `Always` |
| Network Location | CoreLocation | `CLLocationManager` | Permission required | iOS unifies GPS + network |
| Step Counter | CoreMotion | `CMPedometer` | Permission required | Motion & Fitness permission |
| Activity Recognition | CoreMotion | `CMMotionActivityManager` | Permission required | Motion & Fitness permission |
| Heart Rate / Health | HealthKit | `HKHealthStore` | Permission required | Per-data-type authorization |

### Sensors Not Available on iOS

The following Android sensors have **no public iOS equivalent**:

- **Ambient light** (`TYPE_LIGHT`) — private API only
- **Proximity** (`TYPE_PROXIMITY`) — no API (used internally for call screen dimming)
- **Temperature** (`TYPE_AMBIENT_TEMPERATURE`) — no API
- **Humidity** (`TYPE_RELATIVE_HUMIDITY`) — no API

The agent must handle these gracefully: when a tool requests unavailable sensor
data, the provider returns a structured `ToolResult` explaining the limitation
and suggesting alternatives (e.g., weather API for temperature).

## 3. iOS Permission Model

### 3.1 Permission Categories

iOS groups sensor permissions into distinct authorization domains:

| Domain | Info.plist Key | Authorization API | Granularity |
|---|---|---|---|
| **Location** | `NSLocationWhenInUseUsageDescription` | `CLLocationManager.requestWhenInUseAuthorization()` | When-in-use / Always / Denied |
| **Motion & Fitness** | `NSMotionUsageDescription` | `CMMotionActivityManager.authorizationStatus()` | Authorized / Restricted / Denied |
| **HealthKit** | `NSHealthShareUsageDescription` | `HKHealthStore.requestAuthorization(toShare:read:)` | Per-data-type read/write |

### 3.2 Authorization Lifecycle

```text
┌─────────────┐    request     ┌──────────────────┐
│ Not          │──────────────>│ System Prompt     │
│ Determined   │               │ (first time only) │
└─────────────┘               └────────┬───────────┘
                                       │
                              ┌────────┴────────┐
                              │                 │
                         ┌────▼────┐      ┌─────▼────┐
                         │Authorized│      │ Denied   │
                         └────┬────┘      └─────┬────┘
                              │                 │
                         (use sensor)    (graceful fallback)
                              │                 │
                         ┌────▼──────────┐  ┌───▼──────────────┐
                         │ Can be revoked│  │ Can only change   │
                         │ in Settings   │  │ via Settings app  │
                         └───────────────┘  └──────────────────┘
```

**Key constraints:**

1. iOS shows the permission prompt **exactly once** per domain per app install.
   If the user denies, the app cannot re-prompt — it must guide the user to
   Settings.
2. Location authorization can change at any time (user revokes in Settings,
   or background location expires).
3. Motion & Fitness is binary (authorized or not) — no partial grants.
4. HealthKit is per-data-type — the user can grant step count but deny heart rate.

### 3.3 Permission Strategy

The sensor provider follows a **request-on-first-use** model:

1. **Before any sensor read**, check current authorization status.
2. **If not determined**, request permission. The agent loop pauses until the
   user responds to the system prompt.
3. **If authorized**, proceed with sensor read.
4. **If denied/restricted**, return a `ToolResult` with:
   - `isError: true`
   - `errorCode: .privacyBlocked`
   - Actionable message: "Location access denied. Open Settings > Fawx > Location to enable."
5. **Never silently fail** — every denied sensor read must produce an explicit
   result that the agent can reason about.

### 3.4 Integration with Policy Engine

Sensor access maps to policy tiers:

| Sensor Category | Risk Tier | Approval | Rationale |
|---|---|---|---|
| Motion (accel, gyro, mag) | T0 | None | No permission required, no PII |
| Step count / activity | T1 | None (after OS grant) | Low-risk health data, OS-level consent |
| Location (when-in-use) | T1 | None (after OS grant) | OS prompt is the consent gate |
| Location (always/background) | T2 | One-tap | Continuous tracking needs explicit agent consent |
| HealthKit (heart rate, etc.) | T2 | One-tap | Sensitive health data, per-type OS consent + agent consent |

The PolicyEngine evaluates sensor tools the same way as any other tool — the
iOS permission system acts as an **additional** gate on top of the policy tier.
Both must pass for execution to proceed.

## 4. Protocol Skeleton

### `SensorTypes.swift`

```swift
import Foundation
import CoreLocation
import CoreMotion

// MARK: - Sensor Data Types

public enum SensorType: String, Sendable, Codable, CaseIterable {
    // Motion sensors (no permission required)
    case accelerometer
    case gyroscope
    case magnetometer
    case barometer

    // Location sensors (CLLocationManager permission)
    case location

    // Activity sensors (CMMotionActivity permission)
    case stepCount
    case activityRecognition

    // Health sensors (HealthKit permission)
    case heartRate

    /// Whether this sensor requires explicit OS-level permission.
    public var requiresPermission: Bool {
        switch self {
        case .accelerometer, .gyroscope, .magnetometer, .barometer:
            return false
        case .location, .stepCount, .activityRecognition, .heartRate:
            return true
        }
    }

    /// The permission domain this sensor belongs to.
    public var permissionDomain: PermissionDomain? {
        switch self {
        case .accelerometer, .gyroscope, .magnetometer, .barometer:
            return nil
        case .location:
            return .location
        case .stepCount, .activityRecognition:
            return .motionFitness
        case .heartRate:
            return .healthKit
        }
    }
}

public enum PermissionDomain: String, Sendable, Codable {
    case location
    case motionFitness
    case healthKit
}

public enum PermissionStatus: String, Sendable, Codable {
    case notDetermined
    case authorized
    case denied
    case restricted
}

public struct SensorReading: Sendable, Codable {
    public let sensorType: SensorType
    public let timestamp: Date
    public let values: [String: Double]
    public let accuracy: SensorAccuracy?

    public init(
        sensorType: SensorType,
        timestamp: Date = Date(),
        values: [String: Double],
        accuracy: SensorAccuracy? = nil
    ) {
        self.sensorType = sensorType
        self.timestamp = timestamp
        self.values = values
        self.accuracy = accuracy
    }
}

public enum SensorAccuracy: String, Sendable, Codable {
    case low
    case medium
    case high
}

public enum SensorError: Error, Sendable {
    case notAvailable(SensorType)
    case permissionDenied(SensorType, guidance: String)
    case permissionRestricted(SensorType)
    case readFailed(SensorType, underlying: String)
}
```

### `IOSSensorProvider.swift`

```swift
import Foundation

// MARK: - Sensor Provider Protocol

/// IOSSensorProvider defines the contract for accessing device sensors on iOS.
/// Each concrete provider handles a specific permission domain and set of sensors.
///
/// Design principles:
/// 1. Permission checks are explicit — never silently fail.
/// 2. Unavailable sensors return structured errors the agent can reason about.
/// 3. Providers are Sendable for safe use from AgentExecutor (an actor).
public protocol IOSSensorProvider: Sendable {
    /// The sensor types this provider can serve.
    var supportedSensors: Set<SensorType> { get }

    /// Check the current permission status for the given sensor.
    func permissionStatus(for sensor: SensorType) -> PermissionStatus

    /// Request permission for the given sensor's domain.
    /// Returns the resulting status after the user responds.
    /// This may show a system prompt (blocking until user responds).
    func requestPermission(for sensor: SensorType) async -> PermissionStatus

    /// Read the current value from the sensor.
    /// - Throws: `SensorError` if permission denied, sensor unavailable, or read fails.
    func read(sensor: SensorType) async throws -> SensorReading

    /// Start continuous updates for the given sensor at the specified interval.
    /// Returns an AsyncStream of readings. Cancel the task to stop updates.
    func startUpdates(
        sensor: SensorType,
        interval: TimeInterval
    ) async throws -> AsyncStream<SensorReading>
}

/// Extension providing default permission-check-before-read logic.
extension IOSSensorProvider {
    /// Ensures permission is granted before reading. Requests if not determined.
    /// Returns a ToolResult-compatible error if denied.
    public func readWithPermissionCheck(
        sensor: SensorType
    ) async -> Result<SensorReading, SensorError> {
        // No permission needed for basic motion sensors
        guard sensor.requiresPermission else {
            do {
                let reading = try await read(sensor: sensor)
                return .success(reading)
            } catch let error as SensorError {
                return .failure(error)
            } catch {
                return .failure(.readFailed(sensor, underlying: error.localizedDescription))
            }
        }

        var status = permissionStatus(for: sensor)

        if status == .notDetermined {
            status = await requestPermission(for: sensor)
        }

        switch status {
        case .authorized:
            do {
                let reading = try await read(sensor: sensor)
                return .success(reading)
            } catch let error as SensorError {
                return .failure(error)
            } catch {
                return .failure(.readFailed(sensor, underlying: error.localizedDescription))
            }
        case .denied:
            return .failure(.permissionDenied(
                sensor,
                guidance: "Open Settings > Fawx > \(sensor.permissionDomain?.rawValue ?? "Privacy") to enable access."
            ))
        case .restricted:
            return .failure(.permissionRestricted(sensor))
        case .notDetermined:
            // Should not reach here after requestPermission, but handle gracefully
            return .failure(.permissionDenied(
                sensor,
                guidance: "Permission was not granted. Please try again."
            ))
        }
    }
}
```

### `MotionSensorProvider.swift`

```swift
import Foundation
import CoreMotion

/// Provides access to CoreMotion sensors (accelerometer, gyroscope,
/// magnetometer, barometer, step count, activity recognition).
///
/// Motion sensors (accel, gyro, mag) do not require permission.
/// Step count and activity recognition require Motion & Fitness permission.
public actor MotionSensorProvider: IOSSensorProvider {
    private let motionManager: CMMotionManager
    private let pedometer: CMPedometer
    private let activityManager: CMMotionActivityManager
    private let altimeter: CMAltimeter

    public let supportedSensors: Set<SensorType> = [
        .accelerometer, .gyroscope, .magnetometer, .barometer,
        .stepCount, .activityRecognition
    ]

    public init() {
        self.motionManager = CMMotionManager()
        self.pedometer = CMPedometer()
        self.activityManager = CMMotionActivityManager()
        self.altimeter = CMAltimeter()
    }

    public nonisolated func permissionStatus(for sensor: SensorType) -> PermissionStatus {
        switch sensor {
        case .stepCount, .activityRecognition:
            // CMMotionActivityManager is the gate for Motion & Fitness
            switch CMMotionActivityManager.authorizationStatus() {
            case .notDetermined: return .notDetermined
            case .authorized: return .authorized
            case .denied: return .denied
            case .restricted: return .restricted
            @unknown default: return .restricted
            }
        default:
            // Basic motion sensors don't require permission
            return .authorized
        }
    }

    public func requestPermission(for sensor: SensorType) async -> PermissionStatus {
        guard sensor == .stepCount || sensor == .activityRecognition else {
            return .authorized
        }

        // Triggering a query on CMMotionActivityManager prompts for permission
        return await withCheckedContinuation { continuation in
            activityManager.queryActivityStarting(
                from: Date(),
                to: Date(),
                to: .main
            ) { _, _ in
                continuation.resume(returning: self.permissionStatus(for: sensor))
            }
        }
    }

    public func read(sensor: SensorType) async throws -> SensorReading {
        switch sensor {
        case .accelerometer:
            guard motionManager.isAccelerometerAvailable else {
                throw SensorError.notAvailable(.accelerometer)
            }
            motionManager.startAccelerometerUpdates()
            // Brief delay for first reading
            try await Task.sleep(nanoseconds: 100_000_000)
            guard let data = motionManager.accelerometerData else {
                throw SensorError.readFailed(.accelerometer, underlying: "No data available")
            }
            motionManager.stopAccelerometerUpdates()
            return SensorReading(
                sensorType: .accelerometer,
                values: ["x": data.acceleration.x, "y": data.acceleration.y, "z": data.acceleration.z]
            )

        case .gyroscope:
            guard motionManager.isGyroAvailable else {
                throw SensorError.notAvailable(.gyroscope)
            }
            motionManager.startGyroUpdates()
            try await Task.sleep(nanoseconds: 100_000_000)
            guard let data = motionManager.gyroData else {
                throw SensorError.readFailed(.gyroscope, underlying: "No data available")
            }
            motionManager.stopGyroUpdates()
            return SensorReading(
                sensorType: .gyroscope,
                values: ["x": data.rotationRate.x, "y": data.rotationRate.y, "z": data.rotationRate.z]
            )

        case .magnetometer:
            guard motionManager.isMagnetometerAvailable else {
                throw SensorError.notAvailable(.magnetometer)
            }
            motionManager.startMagnetometerUpdates()
            try await Task.sleep(nanoseconds: 100_000_000)
            guard let data = motionManager.magnetometerData else {
                throw SensorError.readFailed(.magnetometer, underlying: "No data available")
            }
            motionManager.stopMagnetometerUpdates()
            return SensorReading(
                sensorType: .magnetometer,
                values: ["x": data.magneticField.x, "y": data.magneticField.y, "z": data.magneticField.z]
            )

        case .barometer:
            guard CMAltimeter.isRelativeAltitudeAvailable() else {
                throw SensorError.notAvailable(.barometer)
            }
            return await withCheckedContinuation { continuation in
                altimeter.startRelativeAltitudeUpdates(to: .main) { [weak altimeter] data, error in
                    altimeter?.stopRelativeAltitudeUpdates()
                    if let data = data {
                        continuation.resume(returning: SensorReading(
                            sensorType: .barometer,
                            values: [
                                "pressure_kPa": data.pressure.doubleValue,
                                "relative_altitude_m": data.relativeAltitude.doubleValue
                            ]
                        ))
                    } else {
                        continuation.resume(returning: SensorReading(
                            sensorType: .barometer,
                            values: [:],
                            accuracy: .low
                        ))
                    }
                }
            }

        case .stepCount:
            guard CMPedometer.isStepCountingAvailable() else {
                throw SensorError.notAvailable(.stepCount)
            }
            let now = Date()
            let startOfDay = Calendar.current.startOfDay(for: now)
            return try await withCheckedThrowingContinuation { continuation in
                pedometer.queryPedometerData(from: startOfDay, to: now) { data, error in
                    if let data = data {
                        continuation.resume(returning: SensorReading(
                            sensorType: .stepCount,
                            values: [
                                "steps": data.numberOfSteps.doubleValue,
                                "distance_m": data.distance?.doubleValue ?? 0
                            ]
                        ))
                    } else {
                        continuation.resume(throwing: SensorError.readFailed(
                            .stepCount,
                            underlying: error?.localizedDescription ?? "Unknown error"
                        ))
                    }
                }
            }

        case .activityRecognition:
            return try await withCheckedThrowingContinuation { continuation in
                activityManager.queryActivityStarting(
                    from: Date().addingTimeInterval(-10),
                    to: Date(),
                    to: .main
                ) { activities, error in
                    if let activity = activities?.last {
                        var values: [String: Double] = [:]
                        if activity.walking { values["walking"] = 1 }
                        if activity.running { values["running"] = 1 }
                        if activity.cycling { values["cycling"] = 1 }
                        if activity.automotive { values["automotive"] = 1 }
                        if activity.stationary { values["stationary"] = 1 }
                        continuation.resume(returning: SensorReading(
                            sensorType: .activityRecognition,
                            values: values
                        ))
                    } else {
                        continuation.resume(throwing: SensorError.readFailed(
                            .activityRecognition,
                            underlying: error?.localizedDescription ?? "No activity data"
                        ))
                    }
                }
            }

        default:
            throw SensorError.notAvailable(sensor)
        }
    }

    public func startUpdates(
        sensor: SensorType,
        interval: TimeInterval
    ) async throws -> AsyncStream<SensorReading> {
        // Continuous updates implementation would use CMMotionManager's
        // startAccelerometerUpdates(to:withHandler:) etc.
        // Skeleton — full implementation after G2 gate.
        throw SensorError.notAvailable(sensor)
    }
}
```

### `LocationSensorProvider.swift`

```swift
import Foundation
import CoreLocation

/// Provides access to CoreLocation (GPS, network location).
/// Requires CLLocationManager permission (when-in-use or always).
public actor LocationSensorProvider: IOSSensorProvider {
    private let locationManager: CLLocationManager
    private let delegate: LocationDelegate

    public let supportedSensors: Set<SensorType> = [.location]

    public init() {
        self.locationManager = CLLocationManager()
        self.delegate = LocationDelegate()
        locationManager.delegate = delegate
        locationManager.desiredAccuracy = kCLLocationAccuracyBest
    }

    public nonisolated func permissionStatus(for sensor: SensorType) -> PermissionStatus {
        switch locationManager.authorizationStatus {
        case .notDetermined: return .notDetermined
        case .authorizedWhenInUse, .authorizedAlways: return .authorized
        case .denied: return .denied
        case .restricted: return .restricted
        @unknown default: return .restricted
        }
    }

    public func requestPermission(for sensor: SensorType) async -> PermissionStatus {
        guard permissionStatus(for: sensor) == .notDetermined else {
            return permissionStatus(for: sensor)
        }

        return await withCheckedContinuation { continuation in
            delegate.onAuthorizationChange = { [weak self] in
                guard let self else { return }
                continuation.resume(returning: self.permissionStatus(for: sensor))
            }
            locationManager.requestWhenInUseAuthorization()
        }
    }

    public func read(sensor: SensorType) async throws -> SensorReading {
        guard sensor == .location else {
            throw SensorError.notAvailable(sensor)
        }

        return try await withCheckedThrowingContinuation { continuation in
            delegate.onLocationUpdate = { location in
                continuation.resume(returning: SensorReading(
                    sensorType: .location,
                    values: [
                        "latitude": location.coordinate.latitude,
                        "longitude": location.coordinate.longitude,
                        "altitude_m": location.altitude,
                        "horizontal_accuracy_m": location.horizontalAccuracy,
                        "speed_mps": location.speed
                    ],
                    accuracy: location.horizontalAccuracy < 10 ? .high :
                              location.horizontalAccuracy < 50 ? .medium : .low
                ))
            }
            delegate.onLocationError = { error in
                continuation.resume(throwing: SensorError.readFailed(
                    .location,
                    underlying: error.localizedDescription
                ))
            }
            locationManager.requestLocation()
        }
    }

    public func startUpdates(
        sensor: SensorType,
        interval: TimeInterval
    ) async throws -> AsyncStream<SensorReading> {
        // Continuous location updates via CLLocationManager.startUpdatingLocation()
        // Skeleton — full implementation after G2 gate.
        throw SensorError.notAvailable(sensor)
    }
}

// MARK: - CLLocationManager Delegate

private final class LocationDelegate: NSObject, CLLocationManagerDelegate, @unchecked Sendable {
    var onAuthorizationChange: (() -> Void)?
    var onLocationUpdate: ((CLLocation) -> Void)?
    var onLocationError: ((Error) -> Void)?

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        onAuthorizationChange?()
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let location = locations.last else { return }
        onLocationUpdate?(location)
        onLocationUpdate = nil  // One-shot for requestLocation
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        onLocationError?(error)
        onLocationError = nil
    }
}
```

### `SensorProviderRegistry.swift`

```swift
import Foundation

/// Central registry for all sensor providers. Routes sensor reads to the
/// appropriate provider and converts results to ToolResult for the agent loop.
public actor SensorProviderRegistry {
    private let providers: [IOSSensorProvider]

    public init(providers: [IOSSensorProvider]) {
        self.providers = providers
    }

    /// Convenience initializer with default providers.
    public init() {
        self.providers = [
            MotionSensorProvider(),
            LocationSensorProvider()
        ]
    }

    /// Find the provider that supports the given sensor type.
    public func provider(for sensor: SensorType) -> IOSSensorProvider? {
        providers.first { $0.supportedSensors.contains(sensor) }
    }

    /// Read a sensor and return a ToolResult suitable for the agent loop.
    public func readAsToolResult(sensor: SensorType) async -> ToolResult {
        guard let provider = provider(for: sensor) else {
            return ToolResult(
                text: "Sensor \(sensor.rawValue) is not available on this iOS device.",
                isError: true,
                errorCode: .notConfigured
            )
        }

        let result = await provider.readWithPermissionCheck(sensor: sensor)
        switch result {
        case .success(let reading):
            let formatted = reading.values.map { "\($0.key): \($0.value)" }.sorted().joined(separator: ", ")
            return ToolResult(text: "[\(sensor.rawValue)] \(formatted)")
        case .failure(let error):
            switch error {
            case .notAvailable(let type):
                return ToolResult(
                    text: "Sensor \(type.rawValue) is not available on this device.",
                    isError: true,
                    errorCode: .notConfigured
                )
            case .permissionDenied(_, let guidance):
                return ToolResult(
                    text: guidance,
                    isError: true,
                    errorCode: .privacyBlocked
                )
            case .permissionRestricted(let type):
                return ToolResult(
                    text: "Access to \(type.rawValue) is restricted by device policy.",
                    isError: true,
                    errorCode: .accessDenied
                )
            case .readFailed(let type, let underlying):
                return ToolResult(
                    text: "Failed to read \(type.rawValue): \(underlying)",
                    isError: true,
                    errorCode: .executionFailed
                )
            }
        }
    }

    /// List all available sensors and their current permission status.
    public func sensorStatus() -> [(SensorType, Bool, PermissionStatus)] {
        SensorType.allCases.map { sensor in
            if let provider = provider(for: sensor) {
                return (sensor, true, provider.permissionStatus(for: sensor))
            } else {
                return (sensor, false, .denied)
            }
        }
    }
}
```

## 5. Module Layout Update

The sensor provider adds a new sub-module under the existing structure:

```text
AssistantApp/
  AgentCoreSwift/
    Sources/AgentCore/
      ...existing files...
  PolicyEngineSwift/
    Sources/PolicyEngine/
      ...existing files...
  SensorProviderIOS/              ← NEW
    Sources/SensorProvider/
      SensorTypes.swift
      IOSSensorProvider.swift
      MotionSensorProvider.swift
      LocationSensorProvider.swift
      SensorProviderRegistry.swift
  AutomationAdaptersIOS/
    Sources/AutomationAdapters/
      ...existing files...
  AssistantUI/
    Sources/AssistantUI/
      ...existing files...
```

## 6. Kotlin → Swift Sensor Mapping

| Kotlin source | Swift target | Strategy |
|---|---|---|
| `AndroidSensorProvider` | `IOSSensorProvider` protocol | Redesign: protocol-based with per-domain concrete providers |
| `SensorManager` access | `CMMotionManager` / `CLLocationManager` | Framework substitution |
| `SensorEventListener` | `CLLocationManagerDelegate` / CoreMotion handlers | Delegate/callback pattern |
| Permission checks (Android runtime permissions) | `CLLocationManager.authorizationStatus` / `CMMotionActivityManager.authorizationStatus()` | iOS permission lifecycle (one-shot prompt) |
| `TYPE_*` sensor constants | `SensorType` enum | Unified enum with availability metadata |

## 7. Testing Strategy

See [ios-testing-infrastructure.md](ios-testing-infrastructure.md) §5 for the
sensor provider test plan. Key contracts to test:

1. **Permission flow:** `notDetermined → requestPermission → authorized/denied`
2. **Graceful degradation:** Unavailable sensors produce structured `ToolResult` errors
3. **Registry routing:** Correct provider selected for each `SensorType`
4. **Policy integration:** Sensor tool calls go through PolicyEngine before provider access
5. **Concurrent access:** Multiple sensor reads from actor-isolated executor are safe

### Mock Sensor Provider (for test harness)

```swift
/// Test double that returns scripted sensor readings.
public actor MockSensorProvider: IOSSensorProvider {
    public let supportedSensors: Set<SensorType>
    private var scriptedReadings: [SensorType: SensorReading]
    private var scriptedPermissions: [SensorType: PermissionStatus]

    public init(
        sensors: Set<SensorType>,
        readings: [SensorType: SensorReading] = [:],
        permissions: [SensorType: PermissionStatus] = [:]
    ) {
        self.supportedSensors = sensors
        self.scriptedReadings = readings
        self.scriptedPermissions = permissions
    }

    public nonisolated func permissionStatus(for sensor: SensorType) -> PermissionStatus {
        // Note: nonisolated access to scriptedPermissions requires careful design.
        // In test harness, configure before use and don't mutate during test.
        .authorized  // Default for tests; override via init
    }

    public func requestPermission(for sensor: SensorType) async -> PermissionStatus {
        scriptedPermissions[sensor] ?? .authorized
    }

    public func read(sensor: SensorType) async throws -> SensorReading {
        guard let reading = scriptedReadings[sensor] else {
            throw SensorError.readFailed(sensor, underlying: "No scripted reading")
        }
        return reading
    }

    public func startUpdates(
        sensor: SensorType,
        interval: TimeInterval
    ) async throws -> AsyncStream<SensorReading> {
        throw SensorError.notAvailable(sensor)
    }

    // MARK: - Test Helpers

    public func setReading(_ reading: SensorReading, for sensor: SensorType) {
        scriptedReadings[sensor] = reading
    }

    public func setPermission(_ status: PermissionStatus, for sensor: SensorType) {
        scriptedPermissions[sensor] = status
    }
}
```

## 8. Open Questions

1. **HealthKit integration depth:** Should MVP include HealthKit sensors, or defer
   to post-MVP? HealthKit authorization is per-data-type and significantly more
   complex than CoreMotion/CoreLocation.
2. **Background sensor access:** Should the agent be able to read sensors when
   the app is backgrounded? This requires `Always` location authorization and
   background modes entitlement — increases App Store review scrutiny.
3. **Sensor data retention:** How long should sensor readings be cached? The
   Android provider reads on-demand; iOS could cache recent readings to reduce
   battery impact.
