
<div align="right">
  <details>
    <summary >🌐 Language</summary>
    <div>
      <div align="center">
        <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=en">English</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=zh-CN">简体中文</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=zh-TW">繁體中文</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=ja">日本語</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=ko">한국어</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=hi">हिन्दी</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=th">ไทย</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=fr">Français</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=de">Deutsch</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=es">Español</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=it">Italiano</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=ru">Русский</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=pt">Português</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=nl">Nederlands</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=pl">Polski</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=ar">العربية</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=fa">فارسی</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=tr">Türkçe</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=vi">Tiếng Việt</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=id">Bahasa Indonesia</a>
        | <a href="https://openaitx.github.io/view.html?user=psi4j&project=sunsetr&lang=as">অসমীয়া</
      </div>
    </div>
  </details>
</div>

# sunsetr

<p align="center">
    <img src=".github/assets/logo.png" alt="sunsetr logo" width="144" />
</p>
<p align="center">
    Automatic blue-light filter for Hyprland, Niri, and everything Wayland
</p>

<p align="center">
  <a href="https://psi4j.github.io/sunsetr/"><img src="https://img.shields.io/badge/Documentation-mdBook-blue" alt="Documentation"></a>
  <a href="https://github.com/psi4j/sunsetr/blob/main/LICENSE"><img src="https://img.shields.io/github/license/psi4j/sunsetr" alt="License"></a>
  <a href="https://github.com/psi4j/sunsetr/releases"><img src="https://img.shields.io/github/v/release/psi4j/sunsetr" alt="Release"></a>
</p>

<p align="center">
  <a href="https://psi4j.github.io/sunsetr/installation.html">Installation</a> | <a href="https://psi4j.github.io/sunsetr/quick-start.html">Quick Start</a> | <a href="https://psi4j.github.io/sunsetr/configuration/">Configuration</a> | <a href="https://psi4j.github.io/sunsetr/commands/">Commands</a>
</p>

![This image was taken using a shader to simulate the effect of sunsetr](.github/assets/sunsetr.png)

Sunsetr is an automatic blue-light filter for Wayland compositors. It automates smooth color-temperature transitions from neutral to warmer tones, designed to reduce eye strain and improve sleep. Sunsetr automatically adjusts your displays based on your geolocation or manual time settings. It can also be used to persist the same color-temperature and gamma indefinitely, and can alternate between various user-defined presets. The controller can integrate with and automate changes in your shell or UI applications via IPC.

## Features

- **Multi-Compositor Support**: Works with Hyprland, Niri, Sway, River, Wayfire, and other Wayland compositors
- **Native Hyprland CTM Backend**: Direct Color Transformation Matrix support for Hyprland
- **Smarter hyprsunset Management**: Add longer, cleaner, and more precise sunset/sunrise transitions to hyprsunset (Hyprland)
- **Smooth Transitions**: Configurable fade effects with adaptive algorithm
- **Preset Management**: Quick switching between configuration profiles (e.g., day, gaming, weekend)
- **Hot Reloading**: Live updates when config files change - no restart needed
- **IPC for Automation**: Unix socket-based IPC for scripting and external integrations
- **Geolocation-based Transitions**: Automatic sunrise/sunset calculation based on your location
- **Interactive City Selection**: Choose from 10,000+ cities worldwide for precise coordinates
- **Automatic Timezone Detection**: Falls back to system timezone for location approximation
- **Universal Wayland Support**: Direct protocol communication on Wayland compositors
- **Smart Defaults**: Works beautifully out-of-the-box with carefully tuned settings
- **Flexible Configuration**: Extensive customization options for power users

## TODO

- [x] Set up AUR package
- [x] Make Nix installation available
- [x] Implement gradual transitions
- [x] Multi-compositor Wayland support
- [x] Geolocation-based transitions
- [x] Implement Hyprland native CTM backend
- [x] Implement IPC for scripting and external integrations
- [ ] Make Fedora Copr installation available
- [ ] Make Debian/Ubuntu installation available

## 💛 Thanks

- to wlsunset, hyprsunset, and redshift for inspiration
- to the Hyprwm team for making Hyprland possible
- to the niri team for making the best Rust-based Wayland compositor
- to the Wayland community for the robust protocol ecosystem
