#if os(iOS)
import AVFoundation
import SwiftUI
import UIKit

struct QRCodeScannerSheet: View {
    let onCancel: () -> Void
    let onCodeScanned: (String) -> Void

    @State private var pastedValue = ""

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: FawxSpacing.paddingLG) {
                Text("Scan the QR code from your Mac to connect this iPhone to Fawx.")
                    .font(FawxTypography.chatBody)
                    .foregroundStyle(Color.fawxTextSecondary)

                QRScannerCameraView(onCodeScanned: onCodeScanned)
                    .frame(height: 320)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .overlay {
                        RoundedRectangle(cornerRadius: 16)
                            .stroke(Color.fawxBorder, lineWidth: 1)
                    }

                VStack(alignment: .leading, spacing: FawxSpacing.paddingSM) {
                    Text("Paste a connection link instead")
                        .font(FawxTypography.sidebarTitle)
                        .foregroundStyle(Color.fawxText)

                    TextField("fawx://connect?host=...", text: $pastedValue)
                        .textFieldStyle(.roundedBorder)

                    Button("Use Pasted Link") {
                        onCodeScanned(pastedValue)
                    }
                    .buttonStyle(.bordered)
                    .disabled(pastedValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                Spacer(minLength: 0)
            }
            .padding(FawxSpacing.paddingLG)
            .background(Color.fawxBackground.ignoresSafeArea())
            .navigationTitle("Scan QR Code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: onCancel)
                }
            }
        }
    }
}

private struct QRScannerCameraView: UIViewControllerRepresentable {
    let onCodeScanned: (String) -> Void

    func makeUIViewController(context: Context) -> QRScannerViewController {
        let controller = QRScannerViewController()
        controller.onCodeScanned = onCodeScanned
        return controller
    }

    func updateUIViewController(_ uiViewController: QRScannerViewController, context: Context) {}
}

@MainActor
private final class QRScannerViewController: UIViewController, @preconcurrency AVCaptureMetadataOutputObjectsDelegate {
    var onCodeScanned: ((String) -> Void)?

    private let captureSession = AVCaptureSession()
    private var previewLayer: AVCaptureVideoPreviewLayer?
    private var hasConfiguredSession = false
    private var didReportCode = false
    private let messageLabel = UILabel()

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = UIColor.black
        configureMessageLabel()
        configureScannerIfPossible()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        previewLayer?.frame = view.bounds
        messageLabel.frame = CGRect(x: 20, y: 20, width: view.bounds.width - 40, height: 60)
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        if captureSession.isRunning {
            captureSession.stopRunning()
        }
    }

    private func configureMessageLabel() {
        messageLabel.textAlignment = .center
        messageLabel.numberOfLines = 0
        messageLabel.textColor = .white
        messageLabel.font = .preferredFont(forTextStyle: .body)
        messageLabel.isHidden = true
        view.addSubview(messageLabel)
    }

    private func configureScannerIfPossible() {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            configureScanner()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
                DispatchQueue.main.async {
                    if granted {
                        self?.configureScanner()
                    } else {
                        self?.showMessage("Camera access is required to scan a pairing QR code.")
                    }
                }
            }
        case .denied, .restricted:
            showMessage("Camera access is unavailable. Paste the connection link instead.")
        @unknown default:
            showMessage("Camera access is unavailable. Paste the connection link instead.")
        }
    }

    private func configureScanner() {
        guard !hasConfiguredSession else {
            if !captureSession.isRunning {
                captureSession.startRunning()
            }
            return
        }

        hasConfiguredSession = true

        guard let videoDevice = AVCaptureDevice.default(for: .video) else {
            showMessage("No camera is available on this device.")
            return
        }

        guard let videoInput = try? AVCaptureDeviceInput(device: videoDevice) else {
            showMessage("Unable to use the camera for QR scanning.")
            return
        }

        if captureSession.canAddInput(videoInput) {
            captureSession.addInput(videoInput)
        } else {
            showMessage("Unable to start the QR scanner.")
            return
        }

        let metadataOutput = AVCaptureMetadataOutput()
        guard captureSession.canAddOutput(metadataOutput) else {
            showMessage("Unable to start the QR scanner.")
            return
        }

        captureSession.addOutput(metadataOutput)
        metadataOutput.setMetadataObjectsDelegate(self, queue: DispatchQueue.main)
        metadataOutput.metadataObjectTypes = [.qr]

        let previewLayer = AVCaptureVideoPreviewLayer(session: captureSession)
        previewLayer.videoGravity = .resizeAspectFill
        previewLayer.frame = view.bounds
        view.layer.insertSublayer(previewLayer, at: 0)
        self.previewLayer = previewLayer
        messageLabel.isHidden = true

        captureSession.startRunning()
    }

    private func showMessage(_ text: String) {
        previewLayer?.removeFromSuperlayer()
        previewLayer = nil
        messageLabel.text = text
        messageLabel.isHidden = false
    }

    func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        guard !didReportCode else {
            return
        }

        guard
            let metadataObject = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
            metadataObject.type == .qr,
            let stringValue = metadataObject.stringValue
        else {
            return
        }

        didReportCode = true
        captureSession.stopRunning()
        onCodeScanned?(stringValue)
    }
}
#endif

#if !os(iOS)
import SwiftUI

struct QRCodeScannerSheet: View {
    let onCancel: () -> Void
    let onCodeScanned: (String) -> Void

    var body: some View {
        EmptyView()
    }
}
#endif
