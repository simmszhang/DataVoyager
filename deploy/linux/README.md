# Linux 构建

跨平台打包验证用：在 Linux 容器中编译 dby 的 Tauri 桌面端。

## 前置

```bash
# 网络受限时用镜像源拉取 rust 镜像（Docker Hub 不可达时）
docker pull docker.m.daocloud.io/library/rust:1-bookworm
docker tag docker.m.daocloud.io/library/rust:1-bookworm rust:1-bookworm

# 构建 builder 镜像（含 webkit2gtk 等 Tauri Linux 依赖）
docker build -t dby-linux-builder -f deploy/linux/Dockerfile .
```

## 编译（验证 Linux release 可构建）

```bash
# 用独立 target 目录，避免与宿主机 Windows 产物混写
docker run --rm -v "$(pwd):/app" -e CARGO_TARGET_DIR=/tmp/dby-target \
  dby-linux-builder cargo build --release -p dby
```

> 已验证：`dby` 在 Linux（rust:1-bookworm + webkit2gtk-4.1）下 release 编译链接通过。

## 完整打包（安装包）

完整 `pnpm tauri build`（产出 AppImage/deb）由 CI 的 `package` 任务执行，见 `.github/workflows/ci.yml`。
