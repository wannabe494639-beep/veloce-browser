# Direct downloads for the Veloce Browser website

The Windows installer is published as a GitHub Release asset. A site button can link straight to that asset; the browser visitor does not need to open GitHub's release page:

```text
https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Setup-x64.exe
```

Linux x86_64 downloads use the same direct-release pattern:

```text
https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Linux-x64.AppImage
https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Linux-x64.deb
```

The AppImage is the portable package; `.deb` is intended for Debian/Ubuntu-based distributions. These links become available only after a successful Linux release workflow. Linux uses WebKitGTK and does not support the bundled WebView2 extensions.

Base44/React Windows button example:

```jsx
<a
  className="download-button"
  href="https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Setup-x64.exe"
  aria-label="Veloce Browser für Windows herunterladen"
>
  Für Windows herunterladen
</a>
```

Linux buttons should use the AppImage or Debian links above. Label the `.deb` for Debian/Ubuntu systems and explain that the AppImage bundles WebKit's media framework. The site should only mark Linux as available after both release assets exist; the `latest/download` URL alone cannot make an unpublished package available.

The URLs become valid after the matching `v*` tag workflow finishes successfully and publishes its assets. The workflow in `.github/workflows/release-windows.yml` creates the Windows installer and source archive, then builds Linux packages. To publish an update, update the app version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, push the changes, and push a matching version tag. Each release includes `VeloceBrowser-Source.zip`, created from committed source only; it excludes the local browser profile and saved website credentials.

This is a download integration, not an embedded browser: Veloce is a native Tauri/WebView2 desktop app and cannot run inside the Base44 page. The landing page should offer the installer and link to the source/release notes separately if desired. Windows may show its usual publisher warning because releases are unsigned unless a code-signing certificate is configured.

