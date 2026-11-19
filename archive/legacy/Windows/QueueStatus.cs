using System.Globalization;
using System.Text.RegularExpressions;

sealed record QueueObservation(string Event, int? Code, int? MembersAhead);

sealed record QueueStatus(Guid? QueueId, int? Ahead, string? EstimatedWait,
    string? PurchaseWindow, string? TimeRemaining, string? Message, int? ErrorId, bool InWaitingRoom)
{
    private static readonly string[] RequiredShareParameters = ["c", "e", "t", "q"];

    public static QueueStatus Parse(string text, int? errorId)
    {
        var flat = Regex.Replace(text, @"\s+", " ");
        var message = Matches(@"no more attempts", flat) ? "No more attempts"
            : Matches(@"the event has ended|this product has sold out", flat) ? "Event ended"
            : Matches(@"(your|this) (queue session|session|queue number|turn) has expired", flat) ? "Session expired"
            : null;
        if (message is not null)
            return new(null, null, null, null, null, message, errorId, false);

        var id = Capture(@"queue id\s*:\s*([0-9a-f-]{36})\b", flat);
        var ahead = Capture(@"(?:members ahead of you|number of users in line ahead of you)\s*:\s*([0-9]+(?:,[0-9]{3})*)\b", flat);
        return new(
            Guid.TryParseExact(id, "D", out var queueId) ? queueId : null,
            int.TryParse(ahead?.Replace(",", ""), NumberStyles.None, CultureInfo.InvariantCulture, out var number) ? number : null,
            Capture(@"(?:estimated time to enter|your estimated wait time is)\s*:\s*((?:(?:more|less) than |about )?\d+\s*(?:hours?|minutes?|seconds?)(?:\s+(?:and\s+)?\d+\s*(?:minutes?|seconds?))?|over an hour|under a minute|calculating)", flat),
            Capture(@"(\d+\s+minutes?)\s+to complete your purchase", flat),
            Capture(@"(?:time remaining|time left)\s*:\s*(\d{1,2}:\d{2}(?::\d{2})?|\d+\s+(?:minutes?|seconds?))", flat),
            null, errorId,
            Matches(@"you are (?:now )?in (?:the |a )?(?:waiting room|(?:virtual )?queue)|in the waiting room", flat));
    }

    public QueueObservation? Observation(bool challengeVisible)
    {
        var state = Message switch
        {
            "No more attempts" => "no_more_attempts",
            "Event ended" => "event_ended",
            "Session expired" => "session_expired",
            _ when ErrorId.HasValue => "queue_error",
            _ when Ahead.HasValue => "queue_position",
            _ when InWaitingRoom => "waiting_room",
            _ when challengeVisible => "captcha_visible",
            _ => null
        };
        return state is null ? null : new(state, ErrorId, Ahead);
    }

    public static bool IsWaitingRoom(Uri url) => url.Scheme == "https" && url.Host == "wr.costco.com";

    public Uri? ResumeUrl(Uri url)
    {
        if (Message is not null || !IsWaitingRoom(url)) return null;
        var parameters = Parameters(url);
        if (parameters.Any(item => Name(item) == "q" && Guid.TryParseExact(Value(item), "D", out _)))
            return url;
        if (QueueId is not { } id) return null;

        return new UriBuilder(url)
        {
            Query = string.Join("&", parameters.Where(item => Name(item) != "q").Append($"q={id:D}"))
        }.Uri;
    }

    public Uri? ShareUrl(Uri url)
    {
        if (Ahead is null || ResumeUrl(url) is not { } resume) return null;
        var parameters = Parameters(resume);
        foreach (var name in RequiredShareParameters)
        {
            var matches = parameters.Where(item => Name(item) == name).ToArray();
            if (matches.Length != 1 || Value(matches[0]).Length == 0) return null;
        }

        return new UriBuilder(resume)
        {
            Path = "/", Fragment = "",
            Query = string.Join("&", parameters.Where(item => Name(item) is "c" or "e" or "t" or "cid" or "q"))
        }.Uri;
    }

    private static string[] Parameters(Uri url) => url.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries);
    private static string Name(string item) => Uri.UnescapeDataString(item.Split('=', 2)[0]);
    private static string Value(string item) => Uri.UnescapeDataString(item.Split('=', 2).ElementAtOrDefault(1) ?? "");
    private static bool Matches(string pattern, string text) =>
        Regex.IsMatch(text, pattern, RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    private static string? Capture(string pattern, string text)
    {
        var match = Regex.Match(text, pattern, RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        return match.Success ? match.Groups[1].Value : null;
    }
}
