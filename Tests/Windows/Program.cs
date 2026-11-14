var id = Guid.Parse("274eb8c4-773b-47d2-b68a-a430522318b6");
var waitingRoom = new Uri("https://wr.costco.com/wait?c=costco&e=drop&t=https%3A%2F%2Fwww.costco.com%2Fa%3Fx%3D1%252B2&cid=en-US&token=private#fragment");
var status = QueueStatus.Parse($"""
    Queue ID: {id}
    You are now in the waiting room
    Members ahead of you: 1,234
    Estimated time to enter: about 1 hour and 12 minutes
    You have 10 minutes to complete your purchase
    Time remaining: 09:41
    """, null);

Check(status is { Ahead: 1234, EstimatedWait: "about 1 hour and 12 minutes",
    PurchaseWindow: "10 minutes", TimeRemaining: "09:41" }, "Parse waiting-room details");
Check(status.ResumeUrl(waitingRoom)?.Query.EndsWith($"&q={id}") == true, "Resume with queue ID");
Check(status.ShareUrl(waitingRoom)?.AbsoluteUri ==
    $"https://wr.costco.com/?c=costco&e=drop&t=https%3A%2F%2Fwww.costco.com%2Fa%3Fx%3D1%252B2&cid=en-US&q={id}",
    "Copy only shareable parameters and preserve encoded target");
Check(status.ShareUrl(new Uri(waitingRoom.AbsoluteUri.Replace("&e=drop", "&e=drop&e=duplicate"))) is null,
    "Reject ambiguous queue links");
Check(status.ResumeUrl(new Uri("https://other.example/?q=" + id)) is null, "Scope resume to Costco");
Check(QueueStatus.Parse("No more attempts left", 42).Observation(true) ==
    new QueueObservation("no_more_attempts", 42, null), "Prioritize queue errors");
Check(QueueStatus.Parse("Waiting for verification", null).Observation(true)?.Event == "captcha_visible",
    "Record a visible manual challenge");
Console.WriteLine("Queue status checks passed.");

static void Check(bool condition, string description)
{
    if (!condition) throw new InvalidOperationException(description);
}
