// Places Windows' notification center as a floating card under Seelen's top bar.
// The Windhawk "Notification Center Styler" lays the center's content out as a W x H card in the
// top-left corner of its window; this moves and shrinks the window to that card, at the top right
// of the monitor's work area (below the top bar), and rounds its corners. Windows keeps the
// center's window opaque, so the window itself must be the card's size.
using System; using System.Text; using System.Threading; using System.Runtime.InteropServices;
static class CenterPlacer {
  const int W = 380, H = 560, Gap = 8, Radius = 12;
  delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
  static WinEventProc proc;
  static void Main() {
    bool created; using (var m = new Mutex(true, @"Local\SeelenCenterPlacer", out created)) { if (!created) return;
      proc = OnEvent;
      // EVENT_OBJECT_SHOW 0x8002 .. EVENT_OBJECT_UNCLOAKED 0x8018 covers show, location change and uncloak.
      SetWinEventHook(0x8002, 0x8018, IntPtr.Zero, proc, 0, 0, 0 /* OUTOFCONTEXT */);
      MSG msg; while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) DispatchMessage(ref msg); }
  }
  static void OnEvent(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time) {
    if (idObject != 0 || hwnd == IntPtr.Zero) return;
    if (ev != 0x8002 && ev != 0x800B && ev != 0x8018) return; // show, location change, uncloaked
    var cls = new StringBuilder(64); GetClassName(hwnd, cls, 64);
    if (cls.ToString() != "Windows.UI.Core.CoreWindow") return;
    uint pid; GetWindowThreadProcessId(hwnd, out pid);
    if (!IsShellExperienceHost(pid)) return;
    RECT r; GetWindowRect(hwnd, out r);
    var mi = new MONITORINFO { cbSize = Marshal.SizeOf(typeof(MONITORINFO)) };
    GetMonitorInfo(MonitorFromWindow(hwnd, 2), ref mi);
    int monH = mi.rcMonitor.B - mi.rcMonitor.T;
    int x = mi.rcWork.R - Gap - W, y = Math.Max(mi.rcWork.T, TopBarBottom(mi.rcMonitor)) + Gap;
    // Windows' own full-height placement at the right edge, or a card this tool placed before.
    bool isCenter = ((r.B - r.T) >= monH * 6 / 10 && r.R == mi.rcMonitor.R) || (r.R - r.L == W && r.B - r.T == H);
    bool placed = r.L == x && r.T == y && r.R - r.L == W && r.B - r.T == H;
    if (!isCenter || placed) return;
    SetWindowPos(hwnd, IntPtr.Zero, x, y, W, H, 0x14);
    SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, W + 1, H + 1, Radius * 2, Radius * 2), true);
  }
  // Bottom edge of Seelen's top bar on this monitor: a seelen-ui window spanning the monitor's
  // width at its top edge. Falls back to the monitor top when there is none.
  static int TopBarBottom(RECT mon) {
    int bottom = mon.T;
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      RECT r; GetWindowRect(h, out r);
      if (r.T != mon.T || r.L > mon.L + 2 || r.R < mon.R - 2 || r.B - r.T > 120 || r.B - r.T < 8) return true;
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (ProcessName(pid).EndsWith(@"\seelen-ui.exe", StringComparison.OrdinalIgnoreCase)) bottom = Math.Max(bottom, r.B);
      return true;
    }, IntPtr.Zero);
    return bottom;
  }
  static string ProcessName(uint pid) {
    IntPtr p = OpenProcess(0x1000, false, pid); if (p == IntPtr.Zero) return "";
    try { var sb = new StringBuilder(1024); uint n = 1024; return QueryFullProcessImageName(p, 0, sb, ref n) ? sb.ToString() : ""; }
    finally { CloseHandle(p); }
  }
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  static bool IsShellExperienceHost(uint pid) {
    return ProcessName(pid).EndsWith(@"\ShellExperienceHost.exe", StringComparison.OrdinalIgnoreCase);
  }
  [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] struct MONITORINFO { public int cbSize; public RECT rcMonitor, rcWork; public uint dwFlags; }
  [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public int x, y; }
  [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint a, uint b, IntPtr m, WinEventProc p, uint pid, uint tid, uint f);
  [DllImport("user32.dll")] static extern int GetMessage(out MSG m, IntPtr h, uint a, uint b);
  [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG m);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint f);
  [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);
  [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int hh, uint f);
  [DllImport("user32.dll")] static extern int SetWindowRgn(IntPtr h, IntPtr r, bool redraw);
  [DllImport("gdi32.dll")] static extern IntPtr CreateRoundRectRgn(int a, int b, int c, int d, int w, int h);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, uint p);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr h, uint f, StringBuilder s, ref uint n);
}
