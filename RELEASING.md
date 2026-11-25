# Veloce release process

For every code change intended for users, publish a new patch version instead of replacing an existing release.

Keep the version synchronized in `package.json`, the root package entry in `package-lock.json`, `src-tauri/Cargo.toml`, the `veloce-browser` package entry in `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json`.

Push the release commit to `main` with `[release]` in its commit message. The GitHub Actions workflow builds the Windows NSIS installer, Linux AppImage and Debian package, and a source ZIP, then attaches them to a new `v<version>` GitHub release.

The website's Windows download should target `https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Setup-x64.exe` so publishing a new GitHub release updates the download without a hard-coded version URL. Keep the visible version in sync with the latest release.

Do not claim a playback-time or memory improvement unless the corresponding behavior was measured in the running app. Build success alone is not a runtime performance test.
