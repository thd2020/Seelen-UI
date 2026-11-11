import type { AppOrFileWegItem } from "../types";
import { convertFileSrc } from "@tauri-apps/api/core";
import { notifications } from "./getters.svelte";
import { type AppNotification, BadgeGlyph, type UserAppWindow } from "@seelen-ui/lib/types";
import type { IconName } from "libs/ui/icons";

function getGlyphIconName(glyph: BadgeGlyph) {
  switch (glyph) {
    case BadgeGlyph.Activity:
      return "TbRefresh";
    case BadgeGlyph.Alarm:
      return "IoAlarm";
    case BadgeGlyph.Alert:
    case BadgeGlyph.Attention:
      return "IoAlert";
    case BadgeGlyph.Available:
      return "var(--color-green-400)";
    case BadgeGlyph.Away:
      return "var(--color-orange-400)";
    case BadgeGlyph.Busy:
      return "var(--color-red-400)";
    case BadgeGlyph.Error:
      return "IoClose";
    case BadgeGlyph.NewMessage:
      return "IoMail";
    case BadgeGlyph.Paused:
      return "IoPause";
    case BadgeGlyph.Playing:
      return "IoPlay";
    case BadgeGlyph.Unavailable:
      return "IoBan";
    case BadgeGlyph.None:
    case BadgeGlyph.Unknown:
      return "";
  }
}

type Badge =
  | { type: "count"; count: number }
  | { type: "color"; color: string }
  | { type: "icon"; name: IconName }
  | { type: "image"; src: string };

export function getBadge(item: AppOrFileWegItem, windows: UserAppWindow[]): Badge | null {
  // The notifications waiting in the notification center come first, so the badge is the real
  // count of what the app sent and drops as they are opened or dismissed.
  const notificationsCount = notifications.value.filter((n) => isFromApp(n, item)).length;
  if (notificationsCount) {
    return { type: "count", count: Math.min(notificationsCount, 99) };
  }

  const badgeValue = windows.find((w) => w.badgeValue)?.badgeValue;
  if (badgeValue) {
    if (typeof badgeValue === "number") {
      return { type: "count", count: Math.min(badgeValue, 99) };
    }

    const iconName = getGlyphIconName(badgeValue);
    if (iconName) {
      if (iconName.startsWith("var")) {
        return { type: "color", color: iconName };
      }
      return { type: "icon", name: iconName as IconName };
    }
  }

  const winWithBadge = windows
    .toSorted((a, b) => b.badgeUpdatedAt - a.badgeUpdatedAt)
    .find((a) => a.badgeIconPath);
  if (winWithBadge && winWithBadge.badgeIconPath && winWithBadge.badgeUpdatedAt) {
    return {
      type: "image",
      src: convertFileSrc(winWithBadge.badgeIconPath) + `?v=${winWithBadge.badgeUpdatedAt}`,
    };
  }

  return null;
}

/**
 * A notification belongs to the dock item when it was sent under the item's app id. Otherwise the
 * app name decides: Windows 10 files tray balloons under Explorer with a per-icon id, and some
 * apps notify under an id other than their window's, so only the name ties them to the app.
 */
function isFromApp(notification: AppNotification, item: AppOrFileWegItem): boolean {
  if (item.umid && notification.appUmid === item.umid) {
    return true;
  }
  const name = notification.appName.trim().toLowerCase();
  if (!name) {
    return false;
  }
  const fileStem = item.path.split(/[\\/]/).pop()?.replace(/\.[^.]*$/, "") ?? "";
  return name === item.displayName.trim().toLowerCase() || name === fileStem.toLowerCase();
}
