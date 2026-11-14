using System.Text.Json;
using System.Text.Json.Serialization;

sealed record SavedSession(Guid Profile, int Number)
{
    public bool Ready { get; set; }
    public Uri? WaitingRoomUrl { get; set; }
}

sealed record SavedBatch(Uri Url, Guid LogId, int NextNumber, int? Selected, List<SavedSession> Sessions);

sealed class BatchStore(string directory)
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web)
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    public string BrowserDirectory => Path.Combine(directory, "Browser Data");
    public string LogDirectory => Path.Combine(directory, "Drop Logs");
    public SavedBatch? ReadBatch() => Read<SavedBatch>("active-batch.json");
    public HashSet<Guid> ReadCleanup() => Read<HashSet<Guid>>("pending-cleanup.json") ?? [];
    public void SaveBatch(SavedBatch? batch) => Write("active-batch.json", batch);
    public void SaveCleanup(HashSet<Guid> profiles) => Write("pending-cleanup.json", profiles);

    public void Log(Guid batch, string eventName, int? session = null, int? code = null, int? membersAhead = null)
    {
        Directory.CreateDirectory(LogDirectory);
        var entry = new LogEntry(DateTimeOffset.UtcNow, eventName, session, code, membersAhead);
        File.AppendAllText(Path.Combine(LogDirectory, $"{batch:D}.jsonl"),
            JsonSerializer.Serialize(entry, Json) + "\n");
    }

    private T? Read<T>(string name)
    {
        try { return JsonSerializer.Deserialize<T>(File.ReadAllText(Path.Combine(directory, name)), Json); }
        catch (FileNotFoundException) { return default; }
        catch (DirectoryNotFoundException) { return default; }
    }

    private void Write<T>(string name, T value)
    {
        Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, name);
        var temporary = path + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(value, Json));
        File.Move(temporary, path, overwrite: true);
    }

    private sealed record LogEntry(DateTimeOffset Timestamp, string Event, int? Session, int? Code, int? MembersAhead);
}
