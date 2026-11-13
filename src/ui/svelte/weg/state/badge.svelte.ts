import type { AppOrFileWegItem } from "../types";
import { convertFileSrc } from "@tauri-apps/api/core";
import { notifications } from "./getters.svelte";
import { type AppNotification, BadgeGlyph, type UserAppWindow } from "@seelen-ui/lib/types";
import type { IconName } from "libs/ui/icons";
import { untrack } from "svelte";
import { SvelteMap } from "svelte/reactivity";

const SEEN_STORAGE_KEY = "notifications-seen-at";

/**
 * When the user last looked at each app, keyed by `appKey`: notifications created up to then are
 * already seen. `Infinity` while one of the app's windows has the focus. Windows keeps an app's
 * notifications in the notification center until they are dismissed there, even after the user
 * read them in the app itself, so the center alone can't tell what is still unread.
 */
const seenAt = new SvelteMap<string, number>(loadSeenAt());

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
  // count of what the app sent. It drops as they are opened or dismissed, and when the user opens
  // the app itself: what it sent until then is seen, even though Windows keeps it in the center.
  const lastSeen = seenAt.get(appKey(item)) ?? 0;
  const notificationsCount = notifications.value.filter(
    (n) => n.date > lastSeen && isFromApp(n, item),
  ).length;
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

/**
 * Call when one of the item's windows gains (`focused = true`) or loses the focus: the user is
 * looking at the app, so every notification it sent until now is seen.
 */
export function markAppSeen(item: AppOrFileWegItem, focused: boolean): void {
  // Called from effects: reading the map there would make them depend on what they write.
  untrack(() => {
    seenAt.set(appKey(item), focused ? Number.POSITIVE_INFINITY : Date.now());
    saveSeenAt();
  });
}

function appKey(item: AppOrFileWegItem): string {
  return (item.umid || item.path).toLowerCase();
}

function loadSeenAt(): Array<[string, number]> {
  try {
    const stored = JSON.parse(localStorage.getItem(SEEN_STORAGE_KEY) ?? "{}");
    return Object.entries(stored).filter((entry): entry is [string, number] => typeof entry[1] === "number");
  } catch {
    return [];
  }
}

/** Keeps what was seen across dock restarts. An app focused right now is saved as seen now. */
function saveSeenAt(): void {
  const now = Date.now();
  const stored: Record<string, number> = {};
  for (const [key, time] of seenAt) {
    stored[key] = Number.isFinite(time) ? time : now;
  }
  localStorage.setItem(SEEN_STORAGE_KEY, JSON.stringify(stored));
}
