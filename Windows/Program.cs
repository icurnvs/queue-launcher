static class Program
{
    [STAThread]
    static void Main()
    {
        ApplicationConfiguration.Initialize();
        using var instance = new Mutex(true, @"Local\QueueLauncher-" + Environment.UserName, out var first);
        if (!first)
        {
            MessageBox.Show("Queue Launcher is already open.", "Queue Launcher");
            return;
        }

        Application.Run(new LauncherForm(Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Queue Launcher")));
    }
}
