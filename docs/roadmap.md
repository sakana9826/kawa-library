# Kawa Library 后续方案

## 欣赏区视频移动到演员文件夹

目标：用户在欣赏区看完视频后，把视频移动到对应演员目录。这个动作必须可预览、可撤销思路清晰，不做静默归档。

### 产品流程

1. 在欣赏区视频卡片增加“归档”入口。
2. 打开归档面板后，左侧显示待归档视频，右侧显示演员列表。
3. 先清理文件名前后缀，只保留番号，例如 `SSNI-025`、`FC2PPV-1234567`。
4. 如果设置中启用联网元数据，则用番号到配置站点搜索，默认站点为 `www.javbus.com`。
5. 解析搜索结果判断演员，并和本地演员文件夹做匹配。
6. 如果联网关闭或解析失败，则只使用本地已有视频番号匹配演员。
7. 用户选择目标演员后生成移动预览：
   - 源文件：`_appreciation\xxx.mp4`
   - 目标目录：`演员文件夹\xxx.mp4`
   - 冲突状态：目标已存在、同番号疑似重复、可移动
8. 用户确认后执行移动，并写入操作日志。
9. 执行完成后刷新欣赏区、演员详情和缓存快照。

### 后端设计

- 新增命令：`preview_archive_to_actor(rootPath, appreciationPath, videoPath, actorPath)`
- 新增命令：`execute_archive_to_actor(rootPath, videoPath, targetPath)`
- 目标路径必须满足：
  - 源文件在欣赏区内
  - 目标目录在库根目录内
  - 目标目录不能是 `_downloading`、`_appreciation`、`_duplicates_review`
  - 目标文件已存在时默认跳过，不覆盖
- 移动后更新 SQLite 中的缓存报告，避免下一次启动再扫全盘。

### 推荐策略

- 第一阶段：从文件名提取番号，提供 JavBus 搜索 URL 和本地番号匹配候选。
- 第二阶段：实现联网搜索解析演员，用户确认后移动。
- 第三阶段：记录用户最近的归档选择，用同一系列/同一前缀辅助推荐。

## 演员头像获取和存储

目标：演员头像不依赖每次扫描目录，头像文件本地持久化，并可手动替换。

设置中新增两个配置：

- 是否启用联网获取头像和作品信息。
- 元数据站点地址，默认 `www.javbus.com`。

### 存储结构

SQLite 新增表：

```sql
CREATE TABLE IF NOT EXISTS actor_profiles (
  actor_path TEXT PRIMARY KEY,
  actor_name TEXT NOT NULL,
  avatar_path TEXT,
  avatar_source TEXT,
  updated_at TEXT NOT NULL
);
```

本地头像文件建议存放在应用数据目录：

```text
app_data_dir\avatars\
```

文件名使用演员路径 hash：

```text
avatars\{actor_path_hash}.jpg
```

### 产品流程

1. 演员详情页头像区域增加“更换头像”入口。
2. 用户从本地选择图片后复制到应用数据目录。
3. SQLite 记录演员路径、头像路径、来源和更新时间。
4. 文件库加载时先读 `actor_profiles`，有头像就显示真实图片，没有头像再显示当前渐变占位。

### 自动获取策略

自动联网抓头像必须由用户在设置中显式启用。

建议分阶段：

1. 第一阶段：本地手动选择头像。
2. 第二阶段：支持从演员文件夹中识别 `avatar.jpg`、`cover.jpg`、`poster.jpg`。
3. 第三阶段：从配置站点搜索演员头像，下载后缓存到本地，并记录来源 URL。

### 缓存失效

- 演员文件夹改名后，`actor_path` 会变化，需要提供“重新绑定头像”的入口。
- 如果头像文件丢失，前端回退到占位头像，同时后端可清理无效记录。
