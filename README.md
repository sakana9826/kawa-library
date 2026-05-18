# Kawa Library

Kawa Library 是一个面向本地视频库整理的桌面应用，基于 **Tauri 2 + React + SQLite** 构建。

它把「文件库浏览」「下载区处理」「欣赏区归档」「查重」放在同一个工作流里，尽量减少手动改名、移动和重复扫描的成本。

## 一句话介绍

适合已经有固定根目录、又希望把视频按演员、番号和下载状态统一整理的人。

## 主要功能

- **文件库**：按演员进入多级视频库，支持头像、封面、排序、筛选和分页/滚动浏览。
- **下载区处理**：自动扫描待处理文件，按规则重命名并移动到欣赏区。
- **欣赏区归档**：按番号联网识别演员，确认后移动到对应演员目录。
- **查重**：按番号统计重复视频，支持忽略重复组和分集后缀显示。
- **封面缓存**：封面由后端生成并缓存，前端只负责按可视区懒加载显示。
- **头像获取**：可配置元数据站点，默认从 `www.javbus.com` 获取并缓存演员头像。
- **本地持久化**：演员目录、封面路径、查重状态、页面偏好等信息写入 SQLite / 本地存储，减少重复扫描。

## 运行前需要

- Windows
- Node.js
- Rust / Cargo
- Visual Studio Build Tools（含 C++ 构建工具）

## 快速开始

安装依赖：

```powershell
npm.cmd install
```

启动开发环境：

```powershell
.\scripts\dev.ps1
```

前端构建：

```powershell
npm.cmd run build
```

Rust 检查：

```powershell
cd src-tauri
cargo check
```

## 打包发布

构建 debug 版本：

```powershell
.\scripts\build-debug.ps1
```

构建正式版：

```powershell
npm run tauri -- build
```

## 默认目录

默认根目录：

```text
D:\KawaLibrary
```

默认工作目录：

```text
_downloading
_appreciation
```

你可以在应用左下角的设置中修改：

- 根目录
- 下载区
- 欣赏区
- 封面截取进度
- 元数据站点
- 是否启用联网识别

## 文件命名规则

应用会优先从文件名里识别番号，例如：

```text
IPZZ-832.mp4
JUR-073-C.mp4
CJOD-138-U.mp4
ABCD-123-UC.mp4
```

标签规则：

- `-C`：字幕
- `-U`：无码
- `-UC`：字幕 + 无码

如果识别不到番号，界面会直接显示文件名。

## FFmpeg

封面生成依赖 `ffmpeg` 和 `ffprobe`。

运行时查找顺序：

1. 内置资源目录 `src-tauri/ffmpeg`
2. 系统 `PATH`

如需随正式版一起打包，把文件放到：

```text
src-tauri\ffmpeg\ffmpeg.exe
src-tauri\ffmpeg\ffprobe.exe
```

## 缓存与数据

应用会持久化这些内容：

- 视频封面
- 演员头像
- 演员目录和归档判断结果
- 查重忽略记录
- 页面排序、筛选、窗口大小等偏好

这些数据由 Tauri 的应用数据/缓存目录管理，不需要每次启动都重新全盘扫描。

## 项目结构

```text
.
├─ src/                 # React 前端
├─ src-tauri/           # Tauri / Rust 后端
│  ├─ ffmpeg/           # 可选：内置 ffmpeg.exe / ffprobe.exe
│  └─ src/
├─ scripts/             # Windows 开发和构建脚本
├─ prototypes/          # 产品原型 HTML
├─ docs/                 # 设计记录和路线图
└─ README.md
```

## 常用命令

```powershell
# 安装依赖
npm.cmd install

# 启动开发环境
.\scripts\dev.ps1

# 前端构建
npm.cmd run build

# Rust 检查
cd src-tauri
cargo check

# 构建 debug 桌面程序
.\scripts\build-debug.ps1

# 构建正式发布版本
npm run tauri -- build
```

## 说明

- debug 版本运行时可能出现终端窗口。
- 正式版按 Windows GUI 应用发布，正常情况下不会弹出终端窗口。
- 文件操作都会先校验是否在配置的根目录内。
- 未完成下载会先跳过，再决定是否清理下载文件夹。