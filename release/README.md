# Release files

**English** | [Français](README.fr.md)

Windows installers, Linux packages, and checksum files are produced for each
release.

- `windows/` contains the NSIS installer, the portable executable, and
  `SHA256SUMS.txt`.
- `linux/` contains the AppImage, DEB, RPM, Linux installation notes, and
  `SHA256SUMS.txt`.

The Windows executables are not signed with an Authenticode certificate.
SmartScreen may warn on first launch.
