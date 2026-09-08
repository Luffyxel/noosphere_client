# Upstream

This directory vendors `tauri-runtime-cef` at commit
`c215d6e52dd507956d9b8cc868906d690dc477c2` from
<https://github.com/byeongsu-hong/tauri-runtime-cef>.

The local patch pins the compatible winit prerelease and exposes the CEF
resource and locale directories through `CefConfig`. The latter is required by
installed Linux packages, where the executable and Chromium resources live in
different directories.
