# Drittanbieter-Erweiterungen in Veloce

Veloce bündelt die unveränderten, entpackten offiziellen Browser-Erweiterungen für die Windows-WebView2-API:

- **uBlock Origin Lite** `2026.1116.1650` – GNU General Public License v3.0. Das Paket enthält außerdem `LICENSE.txt`.
- **SponsorBlock** `6.1.7` – GNU General Public License v3.0; siehe `SponsorBlock-LICENSE.txt`.

Die vollständigen Quellarchive der verwendeten Versionen liegen im Quellprojekt unter `src-tauri/licenses/extensions/`:

- `uBOL-home-2026.1116.1650-source.zip`
- `SponsorBlock-6.1.7-source.zip`

Offizielle Projekt- und Versionsquellen:

- uBlock Origin Lite: https://github.com/uBlockOrigin/uBOL-home/releases/tag/2026.1116.1650
- SponsorBlock: https://github.com/ajayyy/SponsorBlock/releases/tag/6.1.7

Veloce ändert den Erweiterungscode nicht. Die Erweiterungen werden lokal über die von Microsoft WebView2 bereitgestellte Erweiterungs-API installiert; sie kommunizieren entsprechend ihrer eigenen Funktion mit ihren jeweiligen Filterlisten- beziehungsweise SponsorBlock-Diensten.
