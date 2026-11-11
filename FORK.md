# About this fork

This is my build of [Seelen UI](https://github.com/eythaann/Seelen-UI) with fixes and features upstream doesn't have yet, or doesn't want.

## Branches

- `master` mirrors upstream's `master` and carries nothing of mine. Branches meant for an upstream pull request start from here.
- `thd` is what I run: upstream plus my branches merged in, plus the fork-only changes below. It is the default branch.
- `feat/...` and `fix/...` hold one change each, based on `master`, so each can go upstream as its own pull request.

## Fork-only changes on `thd`

- The fork has its own updater signing key. Its public half replaces upstream's in `src/background/utils/updater.rs` and `src/background/utils/integrity/checksums.rs`, and the private half is in the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. A release build checks its own files against this key, so a build signed with upstream's key, or one without a key, won't start.
- The updater reads this fork's latest release instead of upstream's, so upstream releases never replace this build.
- `.github/workflows/fork-release.yml` builds the x64 installer on every push to `thd` and publishes it as the latest release, versioned as upstream's version plus `-thd.<run number>`. Installed copies update from there.

## Extras

- `extras/notification-center/` restyles Windows' own notification center as a compact card under the top bar, for use with notifications left to Windows. See its README.

## Taking in upstream changes

```bash
git fetch upstream
git switch master && git merge --ff-only upstream/master && git push origin master
git switch thd && git merge master && git push origin thd
```

The push to `thd` starts a new release.
