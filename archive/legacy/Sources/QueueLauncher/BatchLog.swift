import Foundation

@MainActor
final class BatchLog {
    nonisolated static var directory: URL {
        URL.applicationSupportDirectory.appending(path: "Queue Launcher/Drop Logs", directoryHint: .isDirectory)
    }

    let id: UUID
    private let directory: URL
    private let encoder = JSONEncoder()
    private let timestamp = ISO8601DateFormatter()
    private(set) var writeFailed = false

    init(id: UUID = UUID(), directory: URL = BatchLog.directory) {
        self.id = id
        self.directory = directory
        timestamp.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    }

    func record(_ event: String, session: Int? = nil, code: Int? = nil, membersAhead: Int? = nil) {
        do {
            var line = try encoder.encode(Entry(
                timestamp: timestamp.string(from: Date()), event: event,
                session: session, code: code, membersAhead: membersAhead
            ))
            line.append(0x0a)

            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let file = directory.appending(path: "\(id.uuidString).jsonl")
            if !FileManager.default.fileExists(atPath: file.path) {
                try line.write(to: file, options: .atomic)
            } else {
                let handle = try FileHandle(forWritingTo: file)
                defer { try? handle.close() }
                try handle.seekToEnd()
                try handle.write(contentsOf: line)
            }
        } catch {
            writeFailed = true
        }
    }

    private struct Entry: Encodable {
        let timestamp: String
        let event: String
        let session: Int?
        let code: Int?
        let membersAhead: Int?
    }
}
