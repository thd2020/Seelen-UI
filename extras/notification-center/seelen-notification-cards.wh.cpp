// ==WindhawkMod==
// @id              seelen-notification-cards
// @name            Seelen notification cards
// @description     One card per notification in Windows 10's notification center, each with its app's icon and name; theme icons for known apps
// @version         1.0
// @author          thd2020
// @include         ShellExperienceHost.exe
// @architecture    x86-64
// @compilerOptions -lole32 -loleaut32 -lruntimeobject
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
// A timer on each of the host's UI threads re-checks the visible cards twice a second, which also
// covers containers the list recycles for other notifications. Every step is guarded: anything
// unexpected skips that card instead of taking down the host.

#include <windows.h>
#undef GetCurrentTime
#include <string>
#include <unordered_map>
#include <vector>
#include <sstream>
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
static std::unordered_map<std::wstring, std::wstring> g_iconByName;  // lowercase app name -> file

static std::wstring Lower(std::wstring s) {
    for (auto& c : s) c = (wchar_t)towlower(c);
    return s;
}

static void LoadIconMap() {
    if (!g_iconByName.empty()) return;
    try {
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

// Adds or updates the icon-and-name row at the top of one notification card.
static void PatchCard(DependencyObject const& item) {
    auto list = FindListAncestor(Media::VisualTreeHelper::GetParent(item));
    if (!list) return;
    auto container = item.try_as<Controls::Primitives::SelectorItem>();
    if (!container) return;
    auto header = list.GroupHeaderContainerFromItemContainer(container);
    if (!header) return;

    std::wstring appName;
    Media::ImageSource logo{nullptr};
    if (auto title = FindNamed(header, L"Title").try_as<Controls::TextBlock>()) appName = title.Text().c_str();
    if (auto image = FindNamed(header, L"StandardImage").try_as<Controls::Image>()) logo = image.Source();
    if (appName.empty()) return;

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
        auto text = row.Children().GetAt(1).try_as<Controls::TextBlock>();
        if (text && text.Text() == appName) return;
        children.RemoveAt(0);
    }

    Controls::Image icon;
    icon.Width(16);
    icon.Height(16);
    icon.VerticalAlignment(VerticalAlignment::Center);
    auto themed = ThemeIcon(appName);
    icon.Source(themed ? themed : logo);

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
    newRow.Children().Append(icon);
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

static void Walk(DependencyObject const& o, int depth) {
    if (!o || depth > 60) return;
    auto cls = winrt::get_class_name(o);
    if (cls == L"ActionCenter.NotificationListViewItem") {
        try { PatchCard(o); } catch (...) {}
        return;
    }
    if (cls == L"ActionCenter.ToastView") {
        try { PatchBanner(o); } catch (...) {}
        return;
    }
    int n = Media::VisualTreeHelper::GetChildrenCount(o);
    for (int i = 0; i < n; i++) Walk(Media::VisualTreeHelper::GetChild(o, i), depth + 1);
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
        LoadIconMap();
        t_state = new ThreadState();
        t_state->timer = DispatcherTimer();
        t_state->timer.Interval(std::chrono::milliseconds(500));
        t_state->timer.Tick([](auto&&, auto&&) {
            try {
                if (auto w = Window::Current())
                    if (auto content = w.Content()) Walk(content, 0);
            } catch (...) {
            }
        });
        t_state->timer.Start();
    } catch (...) {
    }
}

static void WINAPI StopOnThisThread(PVOID) {
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
    for (int i = 0; i < 120; i++) {
        for (HWND h : CoreWindows()) RunFromWindowThread(h, StartOnThisThread);
        Sleep(1000);
    }
    return 0;
}

BOOL Wh_ModInit() { return TRUE; }

void Wh_ModAfterInit() {
    CloseHandle(CreateThread(nullptr, 0, StartLater, nullptr, 0, nullptr));
}

void Wh_ModUninit() {
    for (HWND h : CoreWindows()) RunFromWindowThread(h, StopOnThisThread);
}
