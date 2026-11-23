# Direct Windows download for the Velox Browser website

The Windows installer is published as a GitHub Release asset. A site button can link straight to that asset; the browser visitor does not need to open GitHub's release page:

```text
https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Setup-x64.exe
```

Base44/React button example:

```jsx
<a
  className="download-button"
  href="https://github.com/wannabe494639-beep/veloce-browser/releases/latest/download/VeloceBrowser-Setup-x64.exe"
  aria-label="Veloce Browser für Windows herunterladen"
>
  Für Windows herunterladen
</a>
```

The URL becomes valid after the first `v*` tag build completes and publishes the installer. The workflow in `.github/workflows/release-windows.yml` creates that release on GitHub. To publish an update, update the app version in `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, push the changes, and push a matching version tag such as `v0.1.1`.

This is a download integration, not an embedded browser: Veloce is a native Tauri/WebView2 desktop app and cannot run inside the Base44 page. The landing page should offer the installer and link to the source/release notes separately if desired. Windows may show its usual publisher warning because releases are unsigned unless a code-signing certificate is configured.
