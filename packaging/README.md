# packaging

Distribution recipes and system integration files.

| Path        | What                                                   | Status |
| ----------- | ------------------------------------------------------ | ------ |
| `udev/`     | udev rule giving the logged-in user access to devices  | ready  |
| `flatpak/`  | Flatpak manifest for `io.github.zhappysnake.superdeck` | M7     |
| `appimage/` | AppImage build                                         | M7     |
| `deb/`      | Debian/Ubuntu package                                  | M7     |
| `rpm/`      | Fedora/openSUSE spec                                   | M7     |
| `aur/`      | Arch `PKGBUILD`                                        | M7     |
| `nix/`      | Nix package / flake                                    | M7     |

Native packages (`.deb`, `.rpm`, AUR, Nix) install the udev rule themselves.
Flatpak and AppImage can't, so their first-run screen explains how to install
it on the host.
