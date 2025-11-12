# Installation

<!-- toc -->

Sunsetr can be installed through several methods depending on your distribution and preferences.

## Dependencies

**Optional for Hyprland Users**

- Hyprland >= 0.49.0
- hyprsunset >= v0.2.0 (only if using the hyprsunset backend)

**For Other Wayland Compositors**

- Any Wayland compositor supporting `wlr-gamma-control-unstable-v1` protocol
- No external dependencies - uses native Wayland protocols

---

## Build from Source

### Installing

First, clone the repository:

```bash
git clone https://github.com/psi4j/sunsetr.git &&
cd sunsetr
```

Then install using `cargo-make`:

```bash
# Install cargo-make if you don't have it already
cargo install cargo-make

# Then install system-wide (requires sudo)
cargo make install
# Or install to ~/.local (no sudo)
cargo make install-local
```

Or install manually:

```bash
# Build with cargo
cargo build --release

# Then install manually
sudo cp target/release/sunsetr /usr/local/bin/
```

### Uninstalling

If you used `cargo make install`:

```bash
cargo make uninstall
```

If you manually copied the binary:

```bash
sudo rm /usr/local/bin/sunsetr
```

## Arch Linux (AUR)

[sunsetr](https://aur.archlinux.org/packages/sunsetr), [sunsetr-git](https://aur.archlinux.org/packages/sunsetr-git), and [sunsetr-bin](https://aur.archlinux.org/packages/sunsetr-bin) are available in the AUR:

**sunsetr** (Latest Version)

```bash
paru -S sunsetr
```

**sunsetr-git** (Development Version)

```bash
paru -S sunsetr-git
```

**sunsetr-bin** (Pre-compiled Binary)

```bash
paru -S sunsetr-bin
```

**Recommendation**: Use `sunsetr` for stability, or `sunsetr-git` if you want to help test the latest features.

## NixOS

[sunsetr](https://search.nixos.org/packages?channel=unstable&from=0&size=50&sort=relevance&type=packages&query=sunsetr) is available in nixpkgs unstable.

### NixOS Configuration

Add to your `configuration.nix`:

```nix
environment.systemPackages = with pkgs; [
  sunsetr
];
```

Then rebuild your system:

```bash
sudo nixos-rebuild switch
```

### Imperative Installation

For non-NixOS systems or user-level installation:

```bash
nix-env -iA nixpkgs.sunsetr
```

### Install using nix-shell

Test sunsetr without permanently installing it:

```bash
nix-shell -p sunsetr
```

### Using Flakes

A flake is available for those wanting to use the latest version from `main` without waiting for it to be added to nixpkgs.

Add sunsetr to your flake inputs:

```nix
{
  inputs.sunsetr.url = "github:psi4j/sunsetr";
}
```

Then use it in your configuration:

```nix
{ inputs, pkgs, ... }:
{
  # Install as a system package
  environment.systemPackages = [
    inputs.sunsetr.packages.${pkgs.system}.sunsetr
  ];

  # OR with home-manager
  home.packages = [
    inputs.sunsetr.packages.${pkgs.system}.sunsetr
  ];
}
```

## Other Distributions

### Fedora / RHEL (Copr)

Coming soon! Copr repository support is planned.

### Debian / Ubuntu

Coming soon! `.deb` packages are planned for Debian-based distributions.

### Manual Installation (Any Distribution)

If your distribution isn't listed above, you can build from source:

1. Install Rust and Cargo (if not already installed):

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. Follow the **Build from Source** instructions above.

---

## Verifying Installation

After installation, verify sunsetr is available:

```bash
sunsetr --version
```

You should see output with the current version number.

## Next Steps

Now that sunsetr is installed, you can follow the [Quick Start](quick-start.md) guide to continue setting things up.
