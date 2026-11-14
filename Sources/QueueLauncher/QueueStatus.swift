import Foundation

struct QueueStatus: Equatable {
    var queueID: UUID?
    var ahead: Int?
    var estimatedWait: String?
    var purchaseWindow: String?
    var timeRemaining: String?
    var message: String?
    var errorID: Int?
    var inWaitingRoom = false

    init(pageText: String, errorID: Int? = nil) {
        self.errorID = errorID
        if pageText.range(of: "no more attempts", options: .caseInsensitive) != nil {
            message = "No more attempts"
            return
        }
        if pageText.range(of: #"the event has ended|this product has sold out"#,
                          options: [.regularExpression, .caseInsensitive]) != nil {
            message = "Event ended"
            return
        }
        if pageText.range(of: #"(your|this) (queue session|session|queue number|turn) has expired"#,
                          options: [.regularExpression, .caseInsensitive]) != nil {
            message = "Session expired"
            return
        }

        let flatText = pageText.replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
        if let id = capture(#"queue id\s*:\s*([0-9a-f-]{36})\b"#, in: flatText) {
            queueID = UUID(uuidString: id)
        }
        inWaitingRoom = flatText.range(
            of: #"you are (?:now )?in (?:the |a )?(?:waiting room|(?:virtual )?queue)|in the waiting room"#,
            options: [.regularExpression, .caseInsensitive]
        ) != nil
        if let number = capture(#"(?:members ahead of you|number of users in line ahead of you)\s*:\s*([0-9]+(?:,[0-9]{3})*)\b"#, in: flatText) {
            ahead = Int(number.replacingOccurrences(of: ",", with: ""))
        }
        estimatedWait = capture(
            #"(?:estimated time to enter|your estimated wait time is)\s*:\s*((?:(?:more|less) than |about )?\d+\s*(?:hours?|minutes?|seconds?)(?:\s+(?:and\s+)?\d+\s*(?:minutes?|seconds?))?|over an hour|under a minute|calculating)"#,
            in: flatText
        )
        purchaseWindow = capture(#"(\d+\s+minutes?)\s+to complete your purchase"#, in: flatText)
        timeRemaining = capture(#"(?:time remaining|time left)\s*:\s*(\d{1,2}:\d{2}(?::\d{2})?|\d+\s+(?:minutes?|seconds?))"#,
                                in: flatText)
    }

    func observedState(challengeVisible: Bool) -> String? {
        switch message {
        case "No more attempts": return "no_more_attempts"
        case "Event ended": return "event_ended"
        case "Session expired": return "session_expired"
        default: break
        }
        if errorID != nil { return "queue_error" }
        if ahead != nil { return "queue_position" }
        if inWaitingRoom { return "waiting_room" }
        if challengeVisible { return "captcha_visible" }
        return nil
    }

    func shareURL(from url: URL) -> URL? {
        guard ahead != nil, let resumeURL = resumeURL(from: url),
              var components = URLComponents(url: resumeURL, resolvingAgainstBaseURL: false),
              let items = components.queryItems,
              ["c", "e", "t", "q"].allSatisfy({ name in
                  items.filter { $0.name == name }.count == 1 &&
                      items.contains { $0.name == name && $0.value?.isEmpty == false }
              })
        else { return nil }

        components.path = "/"
        components.fragment = nil
        components.queryItems = items.filter { ["c", "e", "t", "cid", "q"].contains($0.name) }
        return components.url
    }

    func resumeURL(from url: URL) -> URL? {
        guard message == nil, url.scheme == "https", url.host == "wr.costco.com",
              var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else { return nil }

        var items = components.queryItems ?? []
        if items.contains(where: { $0.name == "q" && UUID(uuidString: $0.value ?? "") != nil }) {
            return url
        }
        guard let queueID else { return nil }
        items.removeAll { $0.name == "q" }
        items.append(URLQueryItem(name: "q", value: queueID.uuidString.lowercased()))
        components.queryItems = items
        return components.url
    }

    private func capture(_ pattern: String, in text: String) -> String? {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive),
              let match = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)),
              let range = Range(match.range(at: 1), in: text)
        else { return nil }
        return String(text[range])
    }
}

struct SavedBatch: Codable {
    let url: URL
    let sessions: [SavedSession]
    let selection: Int?
    let logID: UUID?
    var nextSessionNumber: Int? = nil
}

struct SavedSession: Codable {
    let storeID: UUID
    var number: Int?
    var ready = false
    var waitingRoomURL: URL?
}
