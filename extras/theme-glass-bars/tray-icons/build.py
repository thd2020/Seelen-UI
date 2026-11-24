"""Writes the hand-drawn tray icons below as SVG files and lists them in apps.json.

Every tray app shares one drawing system: a 24 px grid, 2.4 px rounded strokes in one ink
colour, and a single solid detail per icon. The top bar swaps each tray image for its icon by
matching the start of the button's tooltip. Run this script after editing, Seelen UI reloads
the theme on its own.
"""
import json
import math
import pathlib

INK = "#1b1d22"
HERE = pathlib.Path(__file__).parent
STYLE = HERE.parent / "styles" / "fancy-toolbar.scss"


def rays(count, inner, long, short, start=-90.0):
    parts = []
    for i in range(count):
        a = math.radians(start + i * 360 / count)
        r = long if i % 2 == 0 else short
        x1, y1 = 12 + inner * math.cos(a), 12 + inner * math.sin(a)
        x2, y2 = 12 + r * math.cos(a), 12 + r * math.sin(a)
        parts.append(f"M{x1:.2f} {y1:.2f}L{x2:.2f} {y2:.2f}")
    return f'<path d="{"".join(parts)}"/>'


def dot(x, y, r=1.0):
    return f'<circle cx="{x}" cy="{y}" r="{r}" fill="{INK}" stroke="none"/>'


ICONS = {
    # two speech bubbles
    "weixin": [
        '<path d="M16.3 9.4C15.9 6.2 13 3.8 9.4 3.8c-3.8 0-6.9 2.6-6.9 5.8 0 1.8 1 3.4 2.5 4.4l-.6 2.3 2.7-1.3c.6.2 1.3.3 2 .3"/>',
        '<path d="M16 10.4c3 0 5.5 2 5.5 4.5 0 1.4-.7 2.6-1.9 3.5l.5 2-2.3-1.1c-.6.2-1.2.2-1.8.2-3 0-5.5-2-5.5-4.6s2.5-4.5 5.5-4.5z"/>',
        dot(7, 8.6, .95), dot(11.8, 8.6, .95), dot(14, 14.6, .85), dot(18, 14.6, .85),
    ],
    # one bubble with a four-petal pinwheel
    "wecom": [
        '<path d="M19.6 10.6c0-3.9-3.6-6.9-8-6.9s-8 3-8 6.9c0 2.2 1.1 4.1 2.8 5.4l-.7 2.8 3-1.6c.9.3 1.9.4 2.9.4"/>',
        dot(17.6, 15.3, 1.15), dot(19.9, 17.6, 1.15), dot(17.6, 19.9, 1.15), dot(15.3, 17.6, 1.15),
        dot(8.7, 10.4, .9), dot(14.5, 10.4, .9),
    ],
    # penguin
    "qq": [
        '<path d="M12 3c-3 0-5.2 2.3-5.2 5.6v1.8C5.4 12.2 4.6 14 4.5 15.6c0 .8.5 1 1 .4l.9-1.1c.4 1.4 1.2 2.6 2.3 3.4l-1.3 1.3c-.4.5 0 1.2 1.1 1.2H11l1-.6 1 .6h2.5c1.1 0 1.5-.7 1.1-1.2l-1.3-1.3c1.1-.8 1.9-2 2.3-3.4l.9 1.1c.5.6 1 .4 1-.4-.1-1.6-.9-3.4-2.3-5.2V8.6C17.2 5.3 15 3 12 3z"/>',
        '<path d="M7.6 12.3c2.8 1.3 6 1.3 8.8 0"/>',
        dot(10.2, 8.4, .95), dot(13.8, 8.4, .95),
    ],
    # sunburst with uneven rays
    "claude": [rays(8, 3.0, 9.6, 7.4, start=-90.0)],
    # three interlaced loops
    "chatgpt": [
        '<ellipse cx="12" cy="12" rx="3.3" ry="8.2"/>',
        '<ellipse cx="12" cy="12" rx="3.3" ry="8.2" transform="rotate(60 12 12)"/>',
        '<ellipse cx="12" cy="12" rx="3.3" ry="8.2" transform="rotate(120 12 12)"/>',
        dot(12, 12, 1.25),
    ],
    # cloud
    "onedrive": [
        '<path d="M6.8 18.6h10.8a3.9 3.9 0 0 0 .5-7.8 5.7 5.7 0 0 0-10.9-1.3 4.6 4.6 0 0 0-.4 9.1z"/>',
        dot(12, 14.4, 1.1),
    ],
    # magnifier
    "everything": [
        '<circle cx="10.5" cy="10.5" r="6.3"/>', '<path d="M15.2 15.2 20.5 20.5"/>', dot(10.5, 10.5, 1.7),
    ],
    # four tiles, one solid
    "powertoys": [
        '<rect x="3.8" y="3.8" width="7" height="7" rx="2"/>',
        '<rect x="13.2" y="3.8" width="7" height="7" rx="2"/>',
        '<rect x="3.8" y="13.2" width="7" height="7" rx="2"/>',
        f'<rect x="13.2" y="13.2" width="7" height="7" rx="2" fill="{INK}"/>',
    ],
    # G ring
    "logitech": ['<path d="M19.2 8.4A7.8 7.8 0 1 0 19.8 13H12.6"/>', dot(12.6, 13, 1.1)],
    # bolt in a ring
    "flclash": [
        '<circle cx="12" cy="12" r="8.6"/>',
        f'<path d="M13.2 5.9 8.3 13h3.4l-.9 5.1 4.9-7.1h-3.4z" fill="{INK}" stroke-width="1.2"/>',
    ],
    # padlock
    "nacc": [
        '<rect x="5" y="10.6" width="14" height="9.8" rx="2.6"/>',
        '<path d="M8.4 10.6V8a3.6 3.6 0 0 1 7.2 0v2.6"/>',
        dot(12, 15.5, 1.4),
    ],
    # shield with a tick
    "security": [
        '<path d="M12 3.2 19 5.8v5.4c0 4.4-2.9 8-7 9.6-4.1-1.6-7-5.2-7-9.6V5.8z"/>',
        '<path d="M8.9 12.1 11.1 14.3 15.3 10"/>',
    ],
    # Phevere's logo letter: a 12-degree lowercase p (stem and bowl), traced from its icon.svg
    "phevere": [
        '<path d="M8.6 3.5L4.9 20.5"/>',
        '<path d="M8.1 5.9C9.7 3.8 12.6 2.8 15.0 2.8C18.6 2.8 20.5 5.2 19.8 8.6C19.0 12.2 16.1 14.6 12.5 14.6C10.1 14.6 7.9 13.7 6.8 11.8"/>',
    ],
}

# Tooltip prefixes as the apps write them, in English and Chinese where they differ.
TOOLTIPS = {
    "weixin": ["Weixin", "WeChat", "微信"],
    "wecom": ["WeCom", "企业微信"],
    "qq": ["QQ"],
    "claude": ["Claude"],
    "chatgpt": ["ChatGPT"],
    "onedrive": ["OneDrive"],
    "everything": ["Everything"],
    "powertoys": ["PowerToys"],
    "logitech": ["Logitech G HUB", "LGHUB"],
    "flclash": ["FlClash"],
    "nacc": ["NACC"],
    "security": ["Windows Security", "Windows 安全中心"],
    "phevere": ["phevere", "Phevere"],
}


def svg(parts):
    body = "".join(parts)
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" '
        f'fill="none" stroke="{INK}" stroke-width="2.4" stroke-linecap="round" '
        f'stroke-linejoin="round">{body}</svg>'
    )


# Executable that owns each hand-drawn icon's tray entry, and its top-bar item id.
EXES = {
    "weixin": "weixin.exe", "wecom": "wxwork.exe", "qq": "qq.exe", "claude": "claude.exe",
    "chatgpt": "chatgpt.exe", "onedrive": "onedrive.exe", "everything": "everything.exe",
    "powertoys": "powertoys.exe", "logitech": "lghub_system_tray.exe", "flclash": "flclash.exe",
    "nacc": "nacccontrol.exe", "security": "securityhealthsystray.exe", "phevere": "phevere.exe",
}

apps_path = HERE / "apps.json"
apps = json.loads(apps_path.read_text(encoding="utf-8")) if apps_path.exists() else {}
for name, parts in ICONS.items():
    (HERE / f"{name}.svg").write_text(svg(parts), encoding="utf-8")
    apps[EXES[name]] = {"icon": f"{name}.svg", "tooltips": TOOLTIPS[name], "item": f"tray-{name}"}
apps_path.write_text(json.dumps(apps, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"{len(ICONS)} icons written; the tray helper rebuilds the stylesheet from apps.json")