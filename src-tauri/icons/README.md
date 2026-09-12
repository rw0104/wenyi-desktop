# 在此放置应用图标（供 `tauri.conf.json` 的 bundle.icon 引用）。

需要的文件（`tauri icon` 可从单张源图生成整套）：

- 32x32.png
- 128x128.png
- 128x128@2x.png
- icon.icns   (macOS)
- icon.ico    (Windows)

生成方式（在 wenyi-desktop/ 目录）：

    npm run tauri icon path/to/icon.png

其中 icon.png 建议为 1024x1024 的方形源图。