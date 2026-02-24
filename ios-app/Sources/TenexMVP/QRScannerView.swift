#if os(iOS)
import SwiftUI
import AVFoundation
import AudioToolbox

struct QRScanResult {
    let nsec: String
    let relay: String?
    let backend: String?
}

struct QRScannerView: UIViewControllerRepresentable {
    let onResult: (QRScanResult) -> Void
    let onError: (String) -> Void

    func makeUIViewController(context: Context) -> QRScannerViewController {
        let controller = QRScannerViewController()
        controller.delegate = context.coordinator
        return controller
    }

    func updateUIViewController(_ uiViewController: QRScannerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(onResult: onResult, onError: onError)
    }

    class Coordinator: NSObject, QRScannerDelegate {
        let onResult: (QRScanResult) -> Void
        let onError: (String) -> Void

        init(onResult: @escaping (QRScanResult) -> Void, onError: @escaping (String) -> Void) {
            self.onResult = onResult
            self.onError = onError
        }

        func didDetectQRCode(_ value: String) {
            // Try URL format first: https://tenex.chat/signin?nsec=...&relay=...&backend=...
            if let components = URLComponents(string: value),
               (components.host == "tenex.chat" || components.scheme == "tenex"),
               let queryItems = components.queryItems,
               let nsec = queryItems.first(where: { $0.name == "nsec" })?.value,
               nsec.hasPrefix("nsec1") {
                let relay = queryItems.first(where: { $0.name == "relay" })?.value
                let backend = queryItems.first(where: { $0.name == "backend" })?.value
                onResult(QRScanResult(nsec: nsec, relay: relay, backend: backend))
                return
            }

            // Try JSON payload: {"nsec": "nsec1...", "relay": "wss://..."}
            if let data = value.data(using: .utf8),
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: String],
               let nsec = json["nsec"], nsec.hasPrefix("nsec1") {
                onResult(QRScanResult(nsec: nsec, relay: json["relay"], backend: json["backend"]))
                return
            }

            // Plain nsec string
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.hasPrefix("nsec1") {
                onResult(QRScanResult(nsec: trimmed, relay: nil, backend: nil))
                return
            }

            onError("Unrecognized QR code")
        }

        func didFailWithError(_ error: String) {
            onError(error)
        }
    }
}

protocol QRScannerDelegate: AnyObject {
    func didDetectQRCode(_ value: String)
    func didFailWithError(_ error: String)
}

class QRScannerViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    weak var delegate: QRScannerDelegate?
    private var captureSession: AVCaptureSession?
    private var previewLayer: AVCaptureVideoPreviewLayer?
    private var hasProcessedCode = false

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        checkCameraPermission()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        previewLayer?.frame = view.bounds
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        captureSession?.stopRunning()
    }

    private func checkCameraPermission() {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            setupCamera()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
                DispatchQueue.main.async {
                    if granted {
                        self?.setupCamera()
                    } else {
                        self?.delegate?.didFailWithError("Camera access denied")
                    }
                }
            }
        case .denied, .restricted:
            delegate?.didFailWithError("Camera access denied. Enable it in Settings.")
        @unknown default:
            delegate?.didFailWithError("Unknown camera permission status")
        }
    }

    private func setupCamera() {
        let session = AVCaptureSession()

        guard let device = AVCaptureDevice.default(for: .video),
              let input = try? AVCaptureDeviceInput(device: device) else {
            delegate?.didFailWithError("Could not access camera")
            return
        }

        guard session.canAddInput(input) else {
            delegate?.didFailWithError("Could not add camera input")
            return
        }
        session.addInput(input)

        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else {
            delegate?.didFailWithError("Could not add metadata output")
            return
        }
        session.addOutput(output)

        output.setMetadataObjectsDelegate(self, queue: .main)
        output.metadataObjectTypes = [.qr]

        let preview = AVCaptureVideoPreviewLayer(session: session)
        preview.videoGravity = .resizeAspectFill
        preview.frame = view.bounds
        view.layer.addSublayer(preview)
        previewLayer = preview

        captureSession = session

        DispatchQueue.global(qos: .userInitiated).async {
            session.startRunning()
        }
    }

    func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput metadataObjects: [AVMetadataObject],
        from connection: AVCaptureConnection
    ) {
        guard !hasProcessedCode else { return }

        guard let object = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
              object.type == .qr,
              let value = object.stringValue else { return }

        hasProcessedCode = true
        captureSession?.stopRunning()
        AudioServicesPlaySystemSound(SystemSoundID(kSystemSoundID_Vibrate))
        delegate?.didDetectQRCode(value)
    }
}
#endif
