# Macrolite

A free, lightweight desktop macro recorder built with Rust.

## Features

- Record mouse movement, clicks, and keyboard input
- Replay recorded actions
- Customizable keyboard shortcuts
- Adjustable playback speed and repetition
- Save and load macros as JSON

## Install on macOS

### Simple installation

1. Download `Macrolite-macOS.zip` from the [latest release](https://github.com/rpradocamargo/macrolite/releases/latest).
2. Double-click the ZIP file to extract `Macrolite.app`.
3. Double-click `Macrolite.app` to launch Macrolite.

You can run Macrolite directly from Downloads or another folder. Moving it to Applications is optional.

macOS may ask you to grant Accessibility and input permissions. Because Macrolite is not notarized by Apple, macOS may also display a security warning.

### Download using Terminal (optional)

    curl -fL https://github.com/rpradocamargo/macrolite/releases/download/v0.1.0/Macrolite-macOS.zip -o ~/Downloads/Macrolite-macOS.zip

Then double-click the downloaded ZIP and launch the app.

## Downloads

Download the latest version from our GitHub Releases page: https://github.com/rpradocamargo/macrolite/releases/latest.

## macOS installation

1. Download the macOS ZIP release.
2. Extract the ZIP archive.
3. Move `Macrolite.app` to Applications or another preferred location.
4. Grant the necessary Accessibility and input permissions.

Macrolite is currently distributed without Apple Developer ID notarization. macOS may display a security warning.

## Donations

Support Macrolite development through PayPal: https://www.paypal.com/donate/?cmd=_s-xclick&hosted_button_id=PVD2ZC47A5HBE.

## Technology

- Rust
- egui / eframe
- rdev
- enigo

## Project status

Macrolite is an independent project. Bug reports and suggestions are welcome.
