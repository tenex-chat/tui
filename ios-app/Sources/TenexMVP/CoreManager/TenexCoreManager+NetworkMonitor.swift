import Foundation
import Network

extension TenexCoreManager {

    func startNetworkMonitoring() {
        let monitor = NWPathMonitor()
        networkPathMonitor = monitor
        lastNetworkPathSatisfied = true

        let queue = DispatchQueue(label: "com.tenex.network-monitor", qos: .utility)
        monitor.pathUpdateHandler = { [weak self] path in
            let isSatisfied = path.status == .satisfied
            Task { @MainActor [weak self] in
                guard let self else { return }
                let wasSatisfied = self.lastNetworkPathSatisfied
                self.lastNetworkPathSatisfied = isSatisfied
                guard !wasSatisfied && isSatisfied else { return }
                self.profiler.logEvent("network path restored, reconnecting", category: .general)
                await self.reconnectAndRefresh()
            }
        }
        monitor.start(queue: queue)
    }

    func stopNetworkMonitoring() {
        (networkPathMonitor as? NWPathMonitor)?.cancel()
        networkPathMonitor = nil
    }
}
