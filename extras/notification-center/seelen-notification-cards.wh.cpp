// ==WindhawkMod==
// @id              seelen-notification-cards
// @name            Seelen notification cards
// @description     Windows 10's notification center as a compact card under the top bar: one card per notification with its app's icon and name, theme icons for known apps
// @version         1.0
// @author          thd2020
// @include         ShellExperienceHost.exe
// @architecture    x86-64
// @compilerOptions -lole32 -loleaut32 -lruntimeobject -lgdi32 -lcomctl32
// ==/WindhawkMod==

// Windows 10 groups the notification center by app: one header row (app logo on an accent plate
// and app name), then that app's cards. This mod gives every card its own header row instead, and
// shows the Seelen theme's icon for apps it knows. The per-app header rows are hidden by the
// Notification Center Styler settings that go with it.
//
// Where the data comes from: a card doesn't carry its app's name, but the list's group header
// does, and ListViewBase.GroupHeaderContainerFromItemContainer finds a card's header. Theme icons
// are PNG files in the notification host's own LocalState\seelen-icons folder (the host runs
// sandboxed and can read only its package folders), listed in map.txt as "app name=file.png".
//
// The icon map loads on the mod's own thread at start. A timer on each of the host's UI
// threads re-checks the visible cards twice a second, which also
// covers containers the list recycles for other notifications. Every step is guarded: anything
// unexpected skips that card instead of taking down the host.

#include <windows.h>
#include <commctrl.h>
#undef GetCurrentTime
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <sstream>
#include <cmath>
#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Storage.h>
#include <winrt/Windows.UI.h>
#include <winrt/Windows.UI.Text.h>
#include <winrt/Windows.UI.Xaml.h>
#include <winrt/Windows.UI.Xaml.Controls.h>
#include <winrt/Windows.UI.Xaml.Media.h>
#include <winrt/Windows.UI.Xaml.Media.Imaging.h>

using namespace winrt::Windows::UI::Xaml;
namespace wf = winrt::Windows::Foundation;

static const wchar_t kHeaderTag[] = L"seelen-card-header";
static std::unordered_map<std::wstring, std::wstring> g_iconByName;
static std::wstring g_closedFile;  // LocalState\\seelen-icons\\closed.txt  // lowercase app name -> file

static std::wstring Lower(std::wstring s) {
    for (auto& c : s) c = (wchar_t)towlower(c);
    return s;
}

static void LoadIconMap() {
    if (!g_iconByName.empty()) return;
    try {
        g_closedFile = std::wstring(winrt::Windows::Storage::ApplicationData::Current().LocalFolder().Path().c_str()) + L"\\seelen-icons\\closed.txt";
        std::wstring path = std::wstring(winrt::Windows::Storage::ApplicationData::Current().LocalFolder().Path().c_str()) +
                            L"\\seelen-icons\\map.txt";
        HANDLE f = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
        if (f == INVALID_HANDLE_VALUE) return;
        std::string bytes;
        char buf[4096];
        DWORD read = 0;
        while (ReadFile(f, buf, sizeof(buf), &read, nullptr) && read > 0) bytes.append(buf, read);
        CloseHandle(f);
        int len = MultiByteToWideChar(CP_UTF8, 0, bytes.data(), (int)bytes.size(), nullptr, 0);
        std::wstring text(len, L'\0');
        MultiByteToWideChar(CP_UTF8, 0, bytes.data(), (int)bytes.size(), text.data(), len);
        std::wstringstream lines(text);
        std::wstring line;
        while (std::getline(lines, line)) {
            if (!line.empty() && line.back() == L'\r') line.pop_back();
            if (!line.empty() && line.front() == 0xFEFF) line.erase(0, 1);
            auto eq = line.find(L'=');
            if (eq == std::wstring::npos || eq == 0) continue;
            g_iconByName[Lower(line.substr(0, eq))] = line.substr(eq + 1);
        }
    } catch (...) {
    }
}

static Media::ImageSource ThemeIcon(std::wstring const& appName) {
    auto it = g_iconByName.find(Lower(appName));
    if (it == g_iconByName.end()) return nullptr;
    try {
        Media::Imaging::BitmapImage bmp;
        bmp.UriSource(wf::Uri(L"ms-appdata:///local/seelen-icons/" + it->second));
        return bmp;
    } catch (...) {
        return nullptr;
    }
}

static FrameworkElement FindNamed(DependencyObject const& root, std::wstring_view name, int depth = 0) {
    if (!root || depth > 30) return nullptr;
    int n = Media::VisualTreeHelper::GetChildrenCount(root);
    for (int i = 0; i < n; i++) {
        auto child = Media::VisualTreeHelper::GetChild(root, i);
        if (auto fe = child.try_as<FrameworkElement>()) {
            if (fe.Name() == name) return fe;
        }
        if (auto found = FindNamed(child, name, depth + 1)) return found;
    }
    return nullptr;
}

static Controls::ListViewBase FindListAncestor(DependencyObject o) {
    for (int i = 0; o && i < 40; i++) {
        if (auto lv = o.try_as<Controls::ListViewBase>()) return lv;
        o = Media::VisualTreeHelper::GetParent(o);
    }
    return nullptr;
}


// ---- placement of the center's window ----
static const int kCardWidth = 396, kCardHeight = 560, kGap = 8, kRadius = 12;

// The window shape for a card with rounded corners of radius r. Window regions have no
// anti-aliasing, so each corner keeps only the pixels the card's rounded corner fully covers.
static HRGN CardRegion(int w, int h, int r) {
    HRGN rgn = CreateRectRgn(0, 0, w, h);
    for (int row = 0; row < r; row++) {
        double dy = r - row;
        double inside = sqrt(r * r - dy * dy > 0 ? r * r - dy * dy : 0);
        int cut = (int)ceil(r - inside);
        if (cut <= 0) continue;
        for (int yy : {row, h - 1 - row}) {
            HRGN left = CreateRectRgn(0, yy, cut, yy + 1), right = CreateRectRgn(w - cut, yy, w, yy + 1);
            CombineRgn(rgn, rgn, left, RGN_DIFF);
            CombineRgn(rgn, rgn, right, RGN_DIFF);
            DeleteObject(left);
            DeleteObject(right);
        }
    }
    return rgn;
}

static bool IsCoreWindow(HWND h) {
    wchar_t cls[64] = L"";
    GetClassName(h, cls, 64);
    return wcscmp(cls, L"Windows.UI.Core.CoreWindow") == 0;
}

using SetWindowPos_t = BOOL(WINAPI*)(HWND, HWND, int, int, int, int, UINT);
static SetWindowPos_t SetWindowPos_Original;
static BOOL WINAPI SetWindowPos_Hook(HWND h, HWND after, int x, int y, int cx, int cy, UINT flags) {
    if (!(flags & SWP_NOSIZE) && cy > 0 && IsCoreWindow(h)) {
        MONITORINFO mi{sizeof(mi)};
        if (GetMonitorInfo(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mi) &&
            cy >= (mi.rcMonitor.bottom - mi.rcMonitor.top) * 6 / 10) {
            // The center opening at full height: make the window the card, under the top bar. The
            // shell draws the window's frosted backdrop and edge line to the window's size, so they
            // then match the card too.
            int cardX = mi.rcWork.right - kGap - kCardWidth, cardY = mi.rcWork.top + kGap;
            BOOL ok = SetWindowPos_Original(h, after, cardX, cardY, cx == 0 ? 0 : kCardWidth, kCardHeight, flags & ~SWP_NOMOVE);
            if (cx != 0) SetWindowRgn(h, CardRegion(kCardWidth, kCardHeight, kRadius), TRUE);
            return ok;
        }
    }
    return SetWindowPos_Original(h, after, x, y, cx, cy, flags);
}

// Cuts the center's window to the card when it has no shape yet. The shell keeps the window at the
// same size from one open to the next, so this can't wait for a resize: it runs when the mod
// attaches to the window and on every timer pass. A window shape lasts until it is replaced.
static void EnsureCardShape(HWND h) {
    if (!IsCoreWindow(h)) return;
    RECT r, box;
    GetWindowRect(h, &r);
    MONITORINFO mi{sizeof(mi)};
    if (!GetMonitorInfo(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mi)) return;
    if (r.bottom - r.top >= (mi.rcMonitor.bottom - mi.rcMonitor.top) * 6 / 10) {
        SetWindowPos_Original(h, nullptr, mi.rcWork.right - kGap - kCardWidth, mi.rcWork.top + kGap, kCardWidth, kCardHeight,
                              SWP_NOZORDER | SWP_NOACTIVATE);
        SetWindowRgn(h, CardRegion(kCardWidth, kCardHeight, kRadius), TRUE);
        return;
    }
    if (r.right - r.left == kCardWidth && r.bottom - r.top == kCardHeight && GetWindowRgnBox(h, &box) == ERROR)
        SetWindowRgn(h, CardRegion(kCardWidth, kCardHeight, kRadius), TRUE);
}

// Adds or updates the icon-and-name row at the top of one notification card.
static bool IsShown(FrameworkElement const& fe) {
    return fe && fe.Visibility() == Visibility::Visible;
}

static std::wstring VisibleText(DependencyObject const& item, std::wstring_view name) {
    auto text = FindNamed(item, name).try_as<Controls::TextBlock>();
    return IsShown(text) ? text.Text().c_str() : L"";
}

static std::wstring DuplicateKey(DependencyObject const& item, std::wstring const& appName) {
    if (appName.empty()) return L"";
    auto title = VisibleText(item, L"Title");
    auto content = VisibleText(item, L"Content");
    if (title.empty() && content.empty()) return L"";

    std::wstring key = Lower(appName);
    for (auto const& part : {title, content, VisibleText(item, L"Attribution")}) {
        key.push_back(L'\x1f');
        key += part;
    }
    return key;
}

// A conversation header: the item has a title but no message and no time.
static bool IsConversationHeader(DependencyObject const& item) {
    auto content = FindNamed(item, L"Content");
    auto attribution = FindNamed(item, L"Attribution");
    return FindNamed(item, L"Title") && !IsShown(content) && !IsShown(attribution);
}

static void PatchCard(DependencyObject const& item, std::unordered_set<std::wstring>& seenNotifications) {
    auto container = item.try_as<UIElement>();
    if (!container) return;
    // Containers are recycled, so set visibility both ways every time.
    bool header = IsConversationHeader(item);
    auto wanted = header ? Visibility::Collapsed : Visibility::Visible;
    if (container.Visibility() != wanted) container.Visibility(wanted);
    if (header) return;

    std::wstring appName;
    Media::ImageSource logo{nullptr};
    if (auto list = FindListAncestor(Media::VisualTreeHelper::GetParent(item))) {
        if (auto selector = item.try_as<Controls::Primitives::SelectorItem>()) {
            auto groupHeader = list.GroupHeaderContainerFromItemContainer(selector);
            if (groupHeader) {
                if (auto title = FindNamed(groupHeader, L"Title").try_as<Controls::TextBlock>())
                    appName = title.Text().c_str();
                if (auto image = FindNamed(groupHeader, L"StandardImage").try_as<Controls::Image>())
                    logo = image.Source();
            }
        }
    }

    auto key = DuplicateKey(item, appName);
    if (!key.empty() && !seenNotifications.insert(key).second) {
        // Hide repeated cards without removing their notifications from Windows' history.
        container.Visibility(Visibility::Collapsed);
        return;
    }

    auto panel = FindNamed(item, L"TextContentPanel").try_as<Controls::StackPanel>();
    if (!panel) return;
    auto children = panel.Children();

    Controls::StackPanel row{nullptr};
    if (children.Size() > 0) {
        if (auto first = children.GetAt(0).try_as<Controls::StackPanel>()) {
            auto tag = first.Tag().try_as<winrt::hstring>();
            if (tag && *tag == kHeaderTag) row = first;
        }
    }
    if (row) {
        // Same app as last time: nothing to do. Recycled for another app: rebuild below.
        Controls::TextBlock text{nullptr};
        if (row.Children().Size() > 1)
            text = row.Children().GetAt(1).try_as<Controls::TextBlock>();
        if (text && text.Text() == appName) return;
        children.RemoveAt(0);
    }

    auto themed = ThemeIcon(appName);
    auto iconSource = themed ? themed : logo;

    Controls::TextBlock name;
    name.Text(appName);
    name.FontSize(12);
    name.FontWeight(winrt::Windows::UI::Text::FontWeights::SemiBold());
    name.Foreground(Media::SolidColorBrush(winrt::Windows::UI::ColorHelper::FromArgb(0x8A, 0, 0, 0)));
    name.Margin(ThicknessHelper::FromLengths(6, 0, 0, 0));
    name.VerticalAlignment(VerticalAlignment::Center);
    name.TextTrimming(TextTrimming::CharacterEllipsis);

    Controls::StackPanel newRow;
    newRow.Orientation(Controls::Orientation::Horizontal);
    newRow.Margin(ThicknessHelper::FromLengths(0, 0, 0, 6));
    newRow.Tag(winrt::box_value(winrt::hstring(kHeaderTag)));
    if (iconSource) {
        Controls::Image icon;
        icon.Width(16);
        icon.Height(16);
        icon.VerticalAlignment(VerticalAlignment::Center);
        icon.Source(iconSource);
        newRow.Children().Append(icon);
    } else {
        Controls::FontIcon icon;
        icon.Glyph(L"\uECAA");
        icon.FontFamily(Media::FontFamily(L"Segoe MDL2 Assets"));
        icon.FontSize(16);
        icon.Width(16);
        icon.Height(16);
        icon.Foreground(name.Foreground());
        icon.VerticalAlignment(VerticalAlignment::Center);
        newRow.Children().Append(icon);
    }
    newRow.Children().Append(name);
    children.InsertAt(0, newRow);
}

// Banners already show the app's logo and name; swap in the theme icon.
static void PatchBanner(DependencyObject const& toast) {
    auto sender = FindNamed(toast, L"SenderName").try_as<Controls::TextBlock>();
    auto logo = FindNamed(toast, L"AppLogo").try_as<Controls::Image>();
    if (!sender || !logo) return;
    std::wstring appName = sender.Text().c_str();
    auto tag = logo.Tag().try_as<winrt::hstring>();
    if (tag && *tag == appName) return;
    if (auto themed = ThemeIcon(appName)) {
        logo.Source(themed);
        logo.Width(16);
        logo.Height(16);
    }
    logo.Tag(winrt::box_value(winrt::hstring(appName)));
}

static void Walk(DependencyObject const& o, int depth, std::unordered_set<std::wstring>& seenNotifications) {
    if (!o || depth > 60) return;
    auto cls = winrt::get_class_name(o);
    if (cls == L"ActionCenter.NotificationListViewItem") {
        try { PatchCard(o, seenNotifications); } catch (...) {}
        return;
    }
    if (cls == L"ActionCenter.ToastView") {
        try { PatchBanner(o); } catch (...) {}
        return;
    }
    int n = Media::VisualTreeHelper::GetChildrenCount(o);
    for (int i = 0; i < n; i++) Walk(Media::VisualTreeHelper::GetChild(o, i), depth + 1, seenNotifications);
}


static void WriteClosedTime() {
    if (g_closedFile.empty()) return;
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULONGLONG ms = ((((ULONGLONG)ft.dwHighDateTime) << 32) | ft.dwLowDateTime) / 10000ULL - 11644473600000ULL;
    std::string text = std::to_string(ms);
    HANDLE f = CreateFileW(g_closedFile.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_ALWAYS, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD w;
    WriteFile(f, text.data(), (DWORD)text.size(), &w, nullptr);
    CloseHandle(f);
}

static LRESULT CALLBACK NoteDeactivation(HWND h, UINT msg, WPARAM wp, LPARAM lp, UINT_PTR, DWORD_PTR) {
    if (msg == WM_WINDOWPOSCHANGED || msg == WM_SHOWWINDOW) EnsureCardShape(h);
    if (msg == WM_ACTIVATE && LOWORD(wp) == WA_INACTIVE) WriteClosedTime();
    if (msg == WM_NCDESTROY) RemoveWindowSubclass(h, NoteDeactivation, 1);
    return DefSubclassProc(h, msg, wp, lp);
}

struct ThreadState {
    DispatcherTimer timer{nullptr};
};
static thread_local ThreadState* t_state = nullptr;
static std::vector<DWORD> g_threads;

static void WINAPI StartOnThisThread(PVOID) {
    try {
        if (t_state) return;
        auto window = Window::Current();
        if (!window) return;
        t_state = new ThreadState();
        t_state->timer = DispatcherTimer();
        t_state->timer.Interval(std::chrono::milliseconds(500));
        t_state->timer.Tick([](auto&&, auto&&) {
            EnumThreadWindows(GetCurrentThreadId(), [](HWND h, LPARAM) -> BOOL { EnsureCardShape(h); return TRUE; }, 0);
            try {
                if (auto w = Window::Current())
                    if (auto content = w.Content()) {
                        std::unordered_set<std::wstring> seenNotifications;
                        Walk(content, 0, seenNotifications);
                    }
            } catch (...) {
            }
        });
        t_state->timer.Start();

        // The center closes when it loses focus. Note the time, so the top-bar bell can tell "the
        // click that just closed it" from "open it" (CenterPlacer reads this file for the bell).
        EnumThreadWindows(GetCurrentThreadId(), [](HWND h, LPARAM) -> BOOL {
            wchar_t cls[64] = L"";
            GetClassName(h, cls, 64);
            if (wcscmp(cls, L"Windows.UI.Core.CoreWindow") == 0) {
                SetWindowSubclass(h, NoteDeactivation, 1, 0);
                EnsureCardShape(h);
            }
            return TRUE;
        }, 0);
    } catch (...) {
    }
}

static void WINAPI StopOnThisThread(PVOID) {
    EnumThreadWindows(GetCurrentThreadId(), [](HWND h, LPARAM) -> BOOL { RemoveWindowSubclass(h, NoteDeactivation, 1); return TRUE; }, 0);
    try {
        if (!t_state) return;
        t_state->timer.Stop();
        delete t_state;
        t_state = nullptr;
    } catch (...) {
    }
}

static bool RunFromWindowThread(HWND hWnd, void(WINAPI* proc)(PVOID)) {
    static const UINT msg = RegisterWindowMessage(L"SeelenNotificationCards_Run");
    DWORD tid = GetWindowThreadProcessId(hWnd, nullptr);
    if (!tid) return false;
    if (tid == GetCurrentThreadId()) {
        proc(nullptr);
        return true;
    }
    HHOOK hook = SetWindowsHookEx(
        WH_CALLWNDPROC,
        [](int code, WPARAM w, LPARAM l) -> LRESULT {
            if (code == HC_ACTION) {
                auto cwp = (const CWPSTRUCT*)l;
                if (cwp->message == msg) ((void(WINAPI*)(PVOID))cwp->lParam)(nullptr);
            }
            return CallNextHookEx(nullptr, code, w, l);
        },
        nullptr, tid);
    if (!hook) return false;
    SendMessage(hWnd, msg, 0, (LPARAM)proc);
    UnhookWindowsHookEx(hook);
    return true;
}

static std::vector<HWND> CoreWindows() {
    std::vector<HWND> wnds;
    EnumWindows(
        [](HWND h, LPARAM l) -> BOOL {
            DWORD pid = 0;
            GetWindowThreadProcessId(h, &pid);
            wchar_t cls[64];
            if (pid == GetCurrentProcessId() && GetClassName(h, cls, 64) &&
                wcscmp(cls, L"Windows.UI.Core.CoreWindow") == 0)
                ((std::vector<HWND>*)l)->push_back(h);
            return TRUE;
        },
        (LPARAM)&wnds);
    return wnds;
}

// The host creates its windows after it starts; keep looking for new ones for a while.
static DWORD WINAPI StartLater(LPVOID) {
    // Load the icon map here, on this thread's own apartment: the UI-thread start runs inside a
    // SendMessage, where Windows refuses outgoing COM calls (RPC_E_CANTCALLOUT_ININPUTSYNCCALL).
    try { winrt::init_apartment(winrt::apartment_type::multi_threaded); } catch (...) {}
    LoadIconMap();
    for (int i = 0; i < 120; i++) {
        for (HWND h : CoreWindows()) RunFromWindowThread(h, StartOnThisThread);
        Sleep(1000);
    }
    return 0;
}

BOOL Wh_ModInit() {
    if (HMODULE user32 = GetModuleHandleW(L"user32.dll"))
        Wh_SetFunctionHook((void*)GetProcAddress(user32, "SetWindowPos"), (void*)SetWindowPos_Hook, (void**)&SetWindowPos_Original);
    return TRUE;
}

void Wh_ModAfterInit() {
    CloseHandle(CreateThread(nullptr, 0, StartLater, nullptr, 0, nullptr));
}

void Wh_ModUninit() {
    for (HWND h : CoreWindows()) RunFromWindowThread(h, StopOnThisThread);
}
