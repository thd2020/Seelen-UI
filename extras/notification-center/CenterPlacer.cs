// Places Windows' notification banners under Seelen's top bar, as rounded cards, and toggles the
// notification center for the top-bar bell. The center itself is sized and shaped by the
// seelen-notification-cards mod inside the notification host; W x H must match its card.
using System; using System.Text; using System.Threading; using System.Runtime.InteropServices;
static class CenterPlacer {
  const int W = 396, H = 560, Gap = 8, Radius = 12;
  const int BannerSide = 16, BannerTop = 71, BannerBottom = 11, BannerRadius = 14;
  static IntPtr lastBanner; static int lastBannerSize;
  delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
  static WinEventProc proc;
  // The top-bar bell opens "seelen-center:toggle", which runs this with that link as argument.
  // Windows' own "open the notification center" never closes it, so the bell does that itself: an
  // open center has focus, and Escape closes it. A click elsewhere can also close the center just
  // before the bell's click arrives; the seelen-notification-cards mod writes the time the center
  // last lost focus to closed.txt, and the bell doesn't reopen a center that closed a moment ago.
  static void Main(string[] args) {
    if (args.Length > 0 && (args[0] == "--toggle" || args[0].StartsWith("seelen-center:", StringComparison.OrdinalIgnoreCase))) {
      IntPtr open = OpenCenter();
      if (open != IntPtr.Zero) {
        SetForegroundWindow(open);
        keybd_event(0x1B, 0, 0, UIntPtr.Zero);        // Escape down
        keybd_event(0x1B, 0, 2, UIntPtr.Zero);        // Escape up
        return;
      }
      long last = 0;
      try {
        string f = System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
          @"Packages\Microsoft.Windows.ShellExperienceHost_cw5n1h2txyewy\LocalState\seelen-icons\closed.txt");
        long.TryParse(System.IO.File.ReadAllText(f).Trim(), out last);
      } catch { }
      if (DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() - last > 600) System.Diagnostics.Process.Start("explorer.exe", "ms-actioncenter:");
      return;
    }
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
    // The center is shaped by the seelen-notification-cards mod inside the host; only banners here.
    bool isCenter = (r.B - r.T) >= monH * 6 / 10;
    if (!isCenter) {
      // Banners: Windows puts their window in the bottom-right corner, above where its taskbar
      // would be. The card sits BannerSide px in from the window's sides, BannerTop from its top and
      // BannerBottom from its bottom, with a square shadow around it. Move the window so the card's
      // top lines up with the center's, and cut the window to the card's rounded shape.
      int bw = r.R - r.L, bh = r.B - r.T;
      if (r.R < mi.rcWork.R - 2 || bh >= monH / 2) return;
      bool bottomRight = r.B >= mi.rcWork.B - 80;
      bool ours = r.T == y - BannerTop;
      if (!bottomRight && !ours) return;
      if (bottomRight) SetWindowPos(hwnd, IntPtr.Zero, r.L, y - BannerTop, 0, 0, 0x15);
      if (lastBanner != hwnd || lastBannerSize != bw * 100000 + bh) {
        lastBanner = hwnd; lastBannerSize = bw * 100000 + bh;
        SetWindowRgn(hwnd, CardRegion(BannerSide, BannerTop, bw - 2 * BannerSide, bh - BannerTop - BannerBottom, BannerRadius), true);
      }
      return;
    }
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
  // The window shape for a card with rounded corners of radius r. Window regions have no
  // anti-aliasing, so each corner keeps only the pixels the card's rounded corner fully covers:
  // the visible edge is then the card's own solid pixels, never the window's dark background.
  // Built row by row, so all four corners are exact mirrors (CreateRoundRectRgn is off by one on
  // two sides).
  static IntPtr CardRegion(int x, int y, int w, int h, int r) {
    IntPtr rgn = CreateRectRgn(x, y, x + w, y + h);
    for (int row = 0; row < r; row++) {
      double dy = r - row;                       // distance from the corner centre to the row's outer edge
      double inside = Math.Sqrt(Math.Max(0, (double)r * r - dy * dy));
      int cut = (int)Math.Ceiling(r - inside);   // pixels to drop at each end of the row
      if (cut <= 0) continue;
      foreach (int yy in new[] { y + row, y + h - 1 - row }) {
        IntPtr left = CreateRectRgn(x, yy, x + cut, yy + 1), right = CreateRectRgn(x + w - cut, yy, x + w, yy + 1);
        CombineRgn(rgn, rgn, left, 4 /* RGN_DIFF */); CombineRgn(rgn, rgn, right, 4);
        DeleteObject(left); DeleteObject(right);
      }
    }
    return rgn;
  }
  [DllImport("gdi32.dll")] static extern IntPtr CreateRectRgn(int a, int b, int c, int d);
  [DllImport("gdi32.dll")] static extern int CombineRgn(IntPtr dst, IntPtr a, IntPtr b, int mode);
  [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
  // The open center: the notification host's window, shown and not cloaked, either at the card's
  // size (the seelen-notification-cards mod sets it) or still at the shell's full height.
  static IntPtr OpenCenter() {
    IntPtr h = IntPtr.Zero;
    while ((h = FindWindowEx(IntPtr.Zero, h, "Windows.UI.Core.CoreWindow", null)) != IntPtr.Zero) {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (!IsShellExperienceHost(pid) || !IsWindowVisible(h)) continue;
      int cloaked; DwmGetWindowAttribute(h, 14, out cloaked, 4);
      RECT r; GetWindowRect(h, out r);
      var mi = new MONITORINFO { cbSize = Marshal.SizeOf(typeof(MONITORINFO)) };
      GetMonitorInfo(MonitorFromWindow(h, 2), ref mi);
      bool card = r.R - r.L == W && r.B - r.T == H;
      if (cloaked == 0 && (card || r.B - r.T >= (mi.rcMonitor.B - mi.rcMonitor.T) * 6 / 10)) return h;
    }
    return IntPtr.Zero;
  }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string t);
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int s);
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
