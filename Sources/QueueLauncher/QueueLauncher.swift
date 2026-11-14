import AppKit
import SwiftUI
import WebKit

@main
struct QueueLauncher: App {
    var body: some Scene {
        Window("Queue Launcher", id: "launcher") {
            LauncherView()
                .frame(minWidth: 720, minHeight: 480)
        }
        .defaultSize(width: 1050, height: 720)
    }
}

@MainActor
private struct LauncherView: View {
    @State private var urlText = ""
    @State private var countText = "5"
    @State private var sessions: [Session] = []
    @State private var selection: Int? = nil
    @State private var nextSessionNumber = 1
    @State private var batchURL: URL?
    @State private var confirmEnd = false
    @State private var restored = false
    @State private var batchLog: BatchLog?
    @State private var logError = false
    @State private var cleanupError: String?
    private let batchKey = "activeBatch"
    private let cleanupKey = "pendingBrowserDataCleanup"

    private var count: Int? {
        guard let value = Int(countText.trimmingCharacters(in: .whitespaces)), (1...50).contains(value)
        else { return nil }
        return value
    }

    private var destination: URL? {
        guard let url = URL(string: urlText.trimmingCharacters(in: .whitespacesAndNewlines))
        else { return nil }
        if url.isFileURL, url.host?.isEmpty != false,
           FileManager.default.isReadableFile(atPath: url.path) {
            return url
        }
        guard let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme),
              let host = url.host, !host.isEmpty else { return nil }
        return url
    }

    private var activeIndex: Int? { sessions.firstIndex { $0.id == selection } }

    private var bestPosition: Int? { sessions.compactMap(\.queueStatus?.ahead).min() }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                TextField("Release URL", text: $urlText, prompt: Text("https://…"))
                    .accessibilityLabel("Release URL")
                Text("Sessions")
                TextField("Sessions", text: $countText)
                    .frame(width: 45)
                    .accessibilityLabel("Session count")
                Button("Open sessions", action: startBatch)
                    .buttonStyle(.borderedProminent)
                    .disabled(destination == nil || count == nil)
            }
            .padding(14)
            .disabled(!sessions.isEmpty)

            Divider()

            if sessions.isEmpty {
                ContentUnavailableView("No open sessions", systemImage: "macwindow")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                HStack(spacing: 0) {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("\(sessions.filter { $0.saved.ready }.count) of \(sessions.count) ready")
                            .font(.subheadline.weight(.semibold))
                            .monospacedDigit()
                            .padding(12)
                        List(selection: $selection) {
                            ForEach(sessions) { session in
                                SessionRow(session: session, bestPosition: bestPosition) {
                                    closeSession(session)
                                }
                                .tag(session.id)
                            }
                        }
                    }
                    .frame(width: 220)
                    Divider()
                    BrowserPane(sessions: sessions, selection: selection)
                }
            }

            Divider()

            HStack {
                Button("End batch") { confirmEnd = true }
                    .disabled(sessions.isEmpty)
                Button("Drop logs", action: openLogs)
                Spacer()
                Button("Back") { activeSession?.webView?.goBack() }
                    .disabled(activeSession?.canGoBack != true)
                Button("Reload") {
                    activeSession?.record("manual_reload")
                    activeSession?.webView?.reload()
                }
                    .disabled(activeSession == nil)
                Divider().frame(height: 16)
                Button("Add session", action: addSession)
                    .disabled(batchURL == nil || sessions.count >= 50)
                Button("Next session", action: showNext)
                    .disabled(sessions.count < 2)
                Button(activeSession?.saved.ready == true ? "Mark pending" : "Mark ready") {
                    guard let activeIndex else { return }
                    let session = sessions[activeIndex]
                    session.saved.ready.toggle()
                    session.record(session.saved.ready ? "marked_ready" : "marked_pending")
                    saveBatch()
                }
                .buttonStyle(.borderedProminent)
                .disabled(activeSession == nil)
            }
            .padding(12)
        }
        .task {
            if !restored {
                restored = true
                restoreBatch()
                let activeIDs = Set(sessions.map(\.saved.storeID))
                let pending = pendingCleanupIDs.subtracting(activeIDs)
                if !pending.isEmpty { Task { await removeBrowserData(pending) } }
            }
            while !Task.isCancelled {
                await withTaskGroup(of: Void.self) { group in
                    for session in sessions {
                        group.addTask { await session.readQueueStatus() }
                    }
                }
                if batchLog?.writeFailed == true { logError = true }
                saveBatch()
                try? await Task.sleep(for: .seconds(3))
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if logError {
                Text("Some drop log entries couldn’t be written.")
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .padding(6)
            }
        }
        .onChange(of: selection) { saveBatch() }
        .alert("Couldn’t clear saved session data", isPresented: Binding(
            get: { cleanupError != nil },
            set: { if !$0 { cleanupError = nil } }
        )) {
            Button("OK") { cleanupError = nil }
        } message: {
            Text(cleanupError ?? "")
        }
        .confirmationDialog("Close all sessions and discard their saved browser data?",
                            isPresented: $confirmEnd, titleVisibility: .visible) {
            Button("End batch", role: .destructive) {
                endBatch()
            }
        }
    }

    private var activeSession: Session? { activeIndex.map { sessions[$0] } }

    private func closeSession(_ session: Session) {
        guard let index = sessions.firstIndex(where: { $0 === session }) else { return }
        if sessions.count == 1 {
            endBatch()
            return
        }
        let storeIDs: Set<UUID> = [session.saved.storeID]
        pendingCleanupIDs.formUnion(storeIDs)
        session.close()
        sessions.remove(at: index)
        if selection == session.id {
            selection = sessions[min(index, sessions.count - 1)].id
        }
        saveBatch()
        Task { await removeBrowserData(storeIDs) }
    }

    private func endBatch() {
        let storeIDs = Set(sessions.map(\.saved.storeID))
        pendingCleanupIDs.formUnion(storeIDs)
        for session in sessions { session.close() }
        batchLog?.record("batch_ended")
        if batchLog?.writeFailed == true { logError = true }
        batchLog = nil
        sessions = []
        selection = nil
        batchURL = nil
        saveBatch()
        Task { await removeBrowserData(storeIDs) }
    }

    private var pendingCleanupIDs: Set<UUID> {
        get {
            Set((UserDefaults.standard.stringArray(forKey: cleanupKey) ?? []).compactMap(UUID.init(uuidString:)))
        }
        nonmutating set {
            UserDefaults.standard.set(newValue.map(\.uuidString).sorted(), forKey: cleanupKey)
        }
    }

    private func removeBrowserData(_ storeIDs: Set<UUID>) async {
        var remaining = storeIDs
        for _ in 0..<12 {
            // SwiftUI and any in-flight status reads can still hold a closing web view.
            try? await Task.sleep(for: .milliseconds(500))
            var retry: Set<UUID> = []
            for storeID in remaining {
                do {
                    try await WKWebsiteDataStore.remove(forIdentifier: storeID)
                } catch {
                    retry.insert(storeID)
                }
            }
            pendingCleanupIDs.subtract(remaining.subtracting(retry))
            guard !retry.isEmpty else { return }
            remaining = retry
            if Task.isCancelled { return }
        }
        cleanupError = "WebKit couldn’t release \(remaining.count) browser data store(s). Cleanup will retry when the app opens again."
    }

    private func openLogs() {
        do {
            let directory = BatchLog.directory
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            NSWorkspace.shared.open(directory)
        } catch {
            logError = true
        }
    }

    private func restoreBatch() {
        guard let data = UserDefaults.standard.data(forKey: batchKey),
              let batch = try? JSONDecoder().decode(SavedBatch.self, from: data),
              !batch.sessions.isEmpty
        else { return }
        urlText = batch.url.absoluteString
        batchURL = batch.url
        countText = String(batch.sessions.count)
        let log = BatchLog(id: batch.logID ?? UUID())
        batchLog = log
        log.record("batch_restored")
        sessions = batch.sessions.enumerated().map { index, saved in
            Session(id: saved.number ?? (index + 1), url: batch.url, log: log, saved: saved, restored: true)
        }
        nextSessionNumber = batch.nextSessionNumber ?? ((sessions.last?.id ?? 0) + 1)
        selection = sessions.first(where: { $0.id == batch.selection })?.id ?? sessions.first?.id
    }

    private func saveBatch() {
        guard let batchURL, !sessions.isEmpty else {
            UserDefaults.standard.removeObject(forKey: batchKey)
            return
        }
        let batch = SavedBatch(
            url: batchURL, sessions: sessions.map(\.saved), selection: selection,
            logID: batchLog?.id, nextSessionNumber: nextSessionNumber
        )
        guard let data = try? JSONEncoder().encode(batch),
              data != UserDefaults.standard.data(forKey: batchKey) else { return }
        UserDefaults.standard.set(data, forKey: batchKey)
    }

    private func startBatch() {
        guard sessions.isEmpty, let destination, let count else { return }
        batchURL = destination
        let log = BatchLog()
        logError = false
        batchLog = log
        log.record("batch_started")
        sessions = (1...count).map { Session(id: $0, url: destination, log: log) }
        nextSessionNumber = count + 1
        selection = sessions.first?.id
        saveBatch()
    }

    private func addSession() {
        guard let batchURL, let batchLog, sessions.count < 50 else { return }
        let session = Session(id: nextSessionNumber, url: batchURL, log: batchLog)
        nextSessionNumber += 1
        sessions.append(session)
        selection = session.id
        saveBatch()
    }

    private func showNext() {
        let nextIndex = activeIndex.map { ($0 + 1) % sessions.count } ?? 0
        selection = sessions[nextIndex].id
    }
}

@MainActor
@Observable
private final class Session: NSObject, Identifiable, WKNavigationDelegate, WKUIDelegate {
    let id: Int
    private(set) var webView: WKWebView?
    var saved: SavedSession
    var queueStatus: QueueStatus?
    var shareURL: URL? {
        guard let url = webView?.url else { return nil }
        return queueStatus?.shareURL(from: url)
    }
    var canGoBack = false
    var errorMessage: String?
    private let initialURL: URL
    private var log: BatchLog?
    private var lastObservation: QueueObservation?

    private struct QueueObservation: Equatable {
        let state: String
        let errorID: Int?
        let membersAhead: Int?
    }

    init(id: Int, url: URL, log: BatchLog,
         saved: SavedSession = SavedSession(storeID: UUID()), restored: Bool = false) {
        self.id = id
        self.saved = saved
        self.log = log
        initialURL = saved.waitingRoomURL ?? url
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = WKWebsiteDataStore(forIdentifier: saved.storeID)
        let webView = WKWebView(frame: .zero, configuration: configuration)
        self.webView = webView
        super.init()
        self.saved.number = id
        webView.navigationDelegate = self
        webView.uiDelegate = self
        record(restored ? "session_restored" : "session_opened")
        loadInitialPage()
    }

    func record(_ event: String, code: Int? = nil, membersAhead: Int? = nil) {
        log?.record(event, session: id, code: code, membersAhead: membersAhead)
    }

    func close() {
        record("session_closed")
        log = nil
        webView?.navigationDelegate = nil
        webView?.uiDelegate = nil
        webView?.stopLoading()
        webView?.removeFromSuperview()
        webView = nil
    }

    func loadInitialPage() {
        guard let webView else { return }
        errorMessage = nil
        if initialURL.isFileURL {
            webView.loadFileURL(initialURL, allowingReadAccessTo: initialURL)
        } else {
            webView.load(URLRequest(url: initialURL))
        }
    }

    func readQueueStatus() async {
        guard let webView, let url = webView.url, url.host == "wr.costco.com" || url.isFileURL,
              errorMessage == nil else {
            queueStatus = nil
            return
        }
        let script = """
        ({
            text: document.body?.innerText ?? '',
            errorID: document.body?.getAttribute('data-errorid') ?? '',
            challengeVisible: Array.from(document.querySelectorAll('iframe')).some(frame =>
                /recaptcha/i.test(frame.title) && frame.getClientRects().length > 0 &&
                getComputedStyle(frame).visibility === 'visible') ? 'yes' : 'no'
        })
        """
        guard let page = (try? await webView.evaluateJavaScript(script)) as? [String: String],
              let text = page["text"], self.webView === webView, webView.url == url, errorMessage == nil else { return }
        let status = QueueStatus(pageText: text, errorID: page["errorID"].flatMap(Int.init))
        if queueStatus != status { queueStatus = status }
        if let state = status.observedState(challengeVisible: page["challengeVisible"] == "yes") {
            let observation = QueueObservation(state: state, errorID: status.errorID, membersAhead: status.ahead)
            if observation != lastObservation {
                lastObservation = observation
                record(state, code: status.errorID, membersAhead: status.ahead)
            }
        }

        if let resumeURL = status.resumeURL(from: url), saved.waitingRoomURL != resumeURL {
            saved.waitingRoomURL = resumeURL
        }
    }

    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
        queueStatus = nil
        lastObservation = nil
        if webView.url?.host == "wr.costco.com" {
            record("waiting_room_page_loaded")
        } else if saved.waitingRoomURL != nil {
            record("left_waiting_room")
        }
        canGoBack = webView.canGoBack
        errorMessage = nil
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        showError(error)
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!,
                 withError error: any Error) {
        showError(error)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        record("web_process_terminated")
        errorMessage = "This session’s web process closed."
    }

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if navigationAction.targetFrame == nil {
            webView.load(navigationAction.request)
        }
        return nil
    }

    private func showError(_ error: any Error) {
        let error = error as NSError
        guard error.domain != NSURLErrorDomain || error.code != NSURLErrorCancelled else { return }
        record(error.domain == NSURLErrorDomain ? "url_load_failed" : "page_load_failed", code: error.code)
        errorMessage = error.localizedDescription
    }
}

private struct BrowserPane: View {
    let sessions: [Session]
    let selection: Int?

    var body: some View {
        ZStack {
            BrowserStack(sessions: sessions, selection: selection)
            if let session = sessions.first(where: { $0.id == selection }),
               let error = session.errorMessage {
                ContentUnavailableView {
                    Label("Page couldn’t load", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(error)
                } actions: {
                    Button("Try again") { session.loadInitialPage() }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(.background)
            }
        }
    }
}

private struct BrowserStack: NSViewRepresentable {
    let sessions: [Session]
    let selection: Int?

    func makeNSView(context: Context) -> NSView { NSView() }

    static func dismantleNSView(_ container: NSView, coordinator: ()) {
        for view in container.subviews { view.removeFromSuperview() }
    }

    func updateNSView(_ container: NSView, context: Context) {
        if sessions.isEmpty {
            for view in container.subviews { view.removeFromSuperview() }
            return
        }

        for session in sessions {
            guard let view = session.webView else { continue }
            view.isHidden = session.id != selection
            if view.superview == nil {
                view.frame = container.bounds
                view.autoresizingMask = [.width, .height]
                container.addSubview(view)
            }
        }
    }
}

private struct SessionRow: View {
    let session: Session
    let bestPosition: Int?
    let onClose: () -> Void
    @State private var copiedURL: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack {
                Label("Session \(session.id)",
                      systemImage: session.saved.ready ? "checkmark.circle.fill" : "circle")
                    .accessibilityValue(session.saved.ready ? "Ready" : "Pending")
                Spacer()
                if let url = session.shareURL {
                    Button {
                        if copyLink(url) {
                            copiedURL = url
                        }
                    } label: {
                        Image(systemName: copiedURL == url ? "checkmark" : "link")
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Copy queue link for session \(session.id)")
                    .help(copiedURL == url ? "Queue link copied" : "Copy queue link")

                    Button {
                        if copyLink(url) { onClose() }
                    } label: {
                        Label("Copy and close", systemImage: "arrow.up.right.square")
                            .labelStyle(.iconOnly)
                    }
                    .buttonStyle(.borderless)
                    .accessibilityLabel("Copy queue link and close session \(session.id)")
                    .help("Copy queue link and close session")
                }
                Button(action: onClose) {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Close session \(session.id)")
                .help("Close session \(session.id)")
            }

            if let status = session.queueStatus {
                if let message = status.message {
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let errorID = status.errorID {
                    Text("Queue-it error \(errorID)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let ahead = status.ahead {
                    HStack {
                        Text("\(ahead.formatted()) ahead")
                            .font(.headline.monospacedDigit())
                        Spacer()
                        if ahead == bestPosition {
                            Text("Best spot")
                                .font(.caption.weight(.semibold))
                        }
                    }
                }
                if let wait = status.estimatedWait {
                    Text("Estimated wait \(wait)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                if let remaining = status.timeRemaining {
                    Text("\(remaining) left")
                        .font(.caption.monospacedDigit())
                } else if let window = status.purchaseWindow {
                    Text("\(window) once admitted")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 3)
    }

    private func copyLink(_ url: URL) -> Bool {
        let clipboard = NSPasteboard.general
        clipboard.clearContents()
        return clipboard.setString(url.absoluteString, forType: .string)
    }
}
