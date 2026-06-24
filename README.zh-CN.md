# NiceSSH

[English](README.md) · [简体中文](README.zh-CN.md)

> 一个跨平台桌面 GUI,用于管理多套 SSH 密钥、Git 身份,以及仓库与账号的绑定关系。
> 不再手写 `~/.ssh/config` 和 `~/.gitconfig`,不再用错账号推送到 GitHub。

[![Release](https://img.shields.io/github/v/release/e9ab98/NiceSSH)](https://github.com/e9ab98/NiceSSH/releases)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue)](#下载)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

## 功能特性

- 🔑 **SSH 密钥管理** — 列出、生成(ED25519 / RSA 4096)、删除、复制公钥
- 👤 **Git 身份管理** — 姓名、邮箱、密钥、目录匹配(`includeIf`)
- 📁 **项目管理** — 注册本地仓库,绑定到指定身份
- 🪄 **目录自动匹配** — `~/work/` 下的仓库自动使用工作身份,零摩擦
- 🪄 **扫描并导入** — 从你手写的 `~/.gitconfig` `includeIf` 块和 `~/.ssh/` 中未被绑定的孤立 key 自动发现身份,一键导入
- 🔍 **可视化的 SSH 配置编辑** — 你手写的配置块按字节原样保留
- 🧪 **SSH 连接测试** — 验证 `git push` 命中的是哪个 GitHub 账号
- ↩️ **变更历史 + 一键回滚** — 对 `~/.ssh/config`、`~/.gitconfig` 以及应用配置的所有改动都会被快照
- 🌗 **跟随系统的深色 / 浅色主题**
- 🌐 **国际化** — 内置英文和简体中文,在「设置 → 语言」中切换
- 🖥️ **原生二进制** — 基于 Tauri 2.x,体积约 5–15 MB,真正以原生应用运行

## 为什么需要它

如果你曾经遇到过:

- 不小心用个人邮箱推到了工作 GitHub
- 花了 30 分钟手改 `~/.ssh/config` 来添加新账号
- 搞不清哪个 SSH 密钥对应哪个 GitHub
- 弄坏了 `~/.gitconfig`,丢了所有 `includeIf` 规则
- 想要一个「提交前预览」视图,看看 SSH 配置最终会变成什么样

NiceSSH 就是为你准备的。

## 下载

在 [**Releases 页面**](https://github.com/e9ab98/NiceSSH/releases/latest) 获取最新版本对应你系统的安装包。

| 平台 | 架构 | 文件 |
|---|---|---|
| **macOS**(推荐) | Intel + Apple Silicon(universal2) | `NiceSSH-v*-macOS-universal.dmg` |
| macOS Intel | x86_64 | `NiceSSH-v*-darwin-x64.dmg` |
| macOS Apple Silicon | aarch64 | `NiceSSH-v*-darwin-arm64.dmg` |
| Windows | x86_64 | `NiceSSH-v*-windows-x64.msi` |
| Linux | x86_64 | `NiceSSH-v*-linux-x64.AppImage` 或 `.deb` |

> **提示:** 滚动构建的 "Latest" 版本(每次 `main` 分支提交都会触发)始终在 <https://github.com/e9ab98/NiceSSH/releases/tag/latest> 提供,适合测试用;稳定使用请选择带版本号的正式发布。

### 安装

**macOS**
1. 双击打开 `.dmg`
2. 把 `NiceSSH.app` 拖入 `/Applications`
3. 首次打开:右键 → 打开(在未签名时绕过 Gatekeeper;1.0 之前)

**Windows**
1. 运行 `.msi` 安装包
2. SmartScreen 可能会提示未签名构建的警告(1.0 之前)—— 点击「更多信息」→「仍要运行」

**Linux(AppImage)**
```bash
chmod +x NiceSSH-v*-linux-x64.AppImage
./NiceSSH-v*-linux-x64.AppImage
```

**Linux(.deb)**
```bash
sudo dpkg -i NiceSSH-v*-linux-x64.deb
```

## 首次使用流程

1. **打开 NiceSSH**。应用默认进入「项目」视图(空)。
2. 进入「身份」→ 点击「**+ 新建身份**」。
   - 标签: `Work`
   - 用户名: `你的名字`
   - 邮箱: `work@company.com`
   - 密钥路径: `~/.ssh/id_work_ed25519`(默认值)
   - 匹配路径: `~/work`(这样 `~/work/` 下所有仓库都会自动使用该身份)
   - 主机别名 / Git 主机: 保持 `github.com`
3. 在新身份卡片上点击「生成密钥」→ 选择 ED25519 → 可选设置口令。
   - 公钥会自动复制到剪贴板。
4. 把公钥添加到你的 GitHub 账号: <https://github.com/settings/keys>
5. 进入「项目」→ 点击「**+ 添加项目**」→ 选择 `~/work/` 下的一个 git 仓库 → 选中 `Work` 身份 → 提交。
6. 该项目的 `.git/config` 已被设置为使用 `id_work_ed25519`,`~/.gitconfig` 也新增了 `includeIf` 块。
7. 点击「测试 SSH」验证 GitHub 把你识别为工作账号。
8. 推送到 GitHub —— 命中的是工作账号,而不是个人账号。

再为 `~/personal/` 添加一个 Personal 身份,基本就齐活了。

## 从手写配置迁移过来

如果你已经手写过 `~/.gitconfig` 的 `includeIf` 块,或者 `~/.ssh/` 里有一些未被绑定的密钥,NiceSSH 可以直接把它们识别为身份并导入。

1. 打开「身份」视图,点击「**扫描并导入**」。
2. NiceSSH 会扫描:
   - `~/.gitconfig` 中所有的 `[includeIf "gitdir:..."]` 块,以及对应的 `~/.gitconfig-<label>` 子文件。
   - `~/.ssh/*.pub` 中所有**未被**任何 `includeIf` 引用的孤立密钥。
3. 弹窗里会展示每个候选身份及其**来源**(provenance),并标记出与现有身份冲突(同名 label 或同 key 路径)的项。
4. 默认会预选所有无冲突的项。取消勾选你不要的,然后点「导入」。

扫描过程**不会写入任何文件**,也不会修改你的 `~/.gitconfig` 和 `~/.ssh/` —— 只有点击「导入」之后才会把候选写入应用配置。

## 实现原理

### 基于目录的自动匹配(`includeIf`)

当你为身份设置了「匹配路径」(例如 `~/work`),NiceSSH 会在 `~/.gitconfig` 末尾追加:

```ini
[includeIf "gitdir:~/work/"]
    path = ~/.gitconfig-work
```

并创建 `~/.gitconfig-work`:

```ini
[user]
    name = 你的名字
    email = work@company.com
[core]
    sshCommand = ssh -i ~/.ssh/id_work_ed25519 -o IdentitiesOnly=yes
```

Git 会自动读取该配置——无需 App 常驻进程,无需守护程序,没有额外跳数。

### 单仓库覆盖

如果某个项目不在你的「匹配路径」下(例如 `~/random/`),你仍然可以在「项目」视图里把它显式绑定到某个身份。这会直接写入该项目的 `.git/config`,覆盖目录级的 `includeIf`。

### SSH 配置的保留策略

NiceSSH 只修改由它自己管理的块,以 `# nicessh-managed` 标记。你手写的块(`Host db-prod`、`Host *` 等)按字节原样保留——只追加新块,绝不覆盖你的文件。

### 变更历史与回滚

对 `~/.ssh/config`、`~/.gitconfig`、`~/.gitconfig-<label>` 以及 `~/.nicessh/config.json` 的每次写入都会快照到 `~/.nicessh/history/`(保留最近 50 条)。打开「历史」视图,选中任意条目,点击「回滚」—— 当前状态会自动先保存一次,再恢复到你选中的版本。

## 开发

NiceSSH 是 [Tauri 2.x](https://tauri.app/) 应用:Rust 后端 + React 18 + TypeScript + Vite 前端。

### 环境要求

- **Node.js 20+** 和 **pnpm 9+**
- **Rust 1.78+**(`rustup install stable`)
- **Tauri 2 的系统依赖** —— 参考 <https://tauri.app/start/prerequisites/>

### 本地搭建

```bash
git clone https://github.com/e9ab98/NiceSSH.git
cd NiceSSH
pnpm install
cargo install tauri-cli --version "^2.0" --locked
cargo tauri dev
```

首次 `cargo build` 会拉取约 1000 个 crate,耗时 5–10 分钟。之后的构建会很快。

### 运行测试

```bash
cd src-tauri
cargo test                          # Rust 单元测试
cargo clippy --all-targets -- -D warnings   # Lint(必须通过)
```

### 为当前平台打包

```bash
cargo tauri build
# → 产出 src-tauri/target/release/bundle/{dmg,msi,appimage,deb}/NiceSSH*
```

### 项目结构

```
.
├── package.json              # 前端依赖
├── vite.config.ts            # Vite 构建配置
├── tailwind.config.ts        # Tailwind,使用 data-theme="dark" 选择器
├── index.html                # 防主题闪烁的内联脚本
├── src/                      # React 前端
│   ├── main.tsx
│   ├── App.tsx               # 侧边栏 + 6 个路由
│   ├── components/           # 通用: Sidebar、ContextMenu、ThemeProvider、ui/
│   ├── views/                # 项目 / 身份 / SSH 密钥 / SSH 配置 / 历史 / 设置
│   ├── features/             # 功能弹窗
│   │   ├── identityForm/     # 新建 / 编辑身份
│   │   ├── keyGenerator/     # ED25519 / RSA 4096 生成 + 口令
│   │   ├── passphraseDialog/ # 给加密 key 走 ssh-add 解锁
│   │   ├── connectionTester/ # ssh -T git@<host> 结果弹窗
│   │   ├── identitySwitcher/ # 切换某仓库的 Git 身份
│   │   ├── scanResults/      # 扫描并导入 弹窗(候选 + 来源)
│   │   └── updateNotification# 应用内更新 toast + 设置 → 更新 tab
│   ├── store/                # Zustand
│   ├── ipc/                  # 类型化的 Tauri 命令封装
│   ├── lib/                  # 框架层胶水(更新器缓存 + 工具函数)
│   ├── i18n/                 # 英文 / 简体中文翻译文件
│   └── hooks/
└── src-tauri/                # Rust 后端
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    └── src/
        ├── main.rs
        ├── lib.rs            # Tauri 构建器 + IPC 命令注册(28 个)
        ├── paths.rs          # ~/ 展开、文件路径解析
        ├── fs_safety.rs      # atomic_write(tmp + rename + chmod)
        ├── history.rs        # 50 条快照索引 + 回滚
        ├── config_store.rs   # ~/.nicessh/config.json
        ├── ssh_config.rs     # 解析 + 序列化(保留用户块)
        ├── git_config.rs     # includeIf 追加 + 每身份子文件
        ├── ssh_keys.rs       # 列出 + 删除
        ├── scanner.rs        # 从既有 includeIf / 孤立 key 中发现身份
        ├── runner.rs         # 带 30s 超时的 exec
        └── commands/         # 28 个 #[tauri::command] 处理函数
            ├── scanner.rs        # scan_existing_identities
            ├── ssh_add_askpass.rs# ssh-add + GUI 口令对话框(Unix setsid)
            └── …
```

### 添加翻译

字符串集中在 `src/i18n/locales/{en,zh-CN}.json`。新增 key 时请同时编辑两个文件。组件通过 `useTranslation()` 钩子读取当前语言(在「设置 → 语言」中选择)。要新增语言,只需添加一份 JSON 文件并在 `src/i18n/index.ts` 中注册即可。

## 安全与数据处理

- **无网络请求**,除非你显式触发(例如 SSH 连接测试)。
- **所有文件系统写入** 都经过 `atomic_write`(临时文件 + rename),即使中途崩溃也不会留下半写状态。
- **口令永不入盘**。它仅保存在解锁对话框的 React state 中,调用 `ssh-add` 一次后即丢弃。
- **SSH 私钥** 仅被应用**读取**(用于计算指纹),不会复制或外发。
- `~/.nicessh/` 目录及其 `history/` 子目录只包含元数据 + 公开配置文件的 before/after diff。**私钥材料永远不会进入任何快照**。

## 更新

NiceSSH 每次启动时检查一次新版本(带 24h 缓存)。当有可用新版本时,会显示一条非阻塞的 toast 提示,提供「立即升级 / 稍后 / ×」三个按钮。点击「立即升级」会在后台下载,下载完成后应用会提示你关闭并重新打开以应用新版本。

也可以在「**设置 → 更新**」中按需检查。该 tab 显示:当前版本、最新可用版本,以及一个「关闭更新通知」的开关。

### 签名与首次升级提示

更新从 GitHub Releases 拉取,并通过 Tauri updater 插件做**签名校验**(`TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 在发布阶段是必填密钥,缺失则发布任务直接失败)。因此你看到的「更新提示」一定对应着真实、经过签名的构建。

**第一次可用的应用内升级路径是 v0.3.0 → v0.3.1**:v0.3.0 落地了整套框架(代码、设置 UI、CI 密钥校验),v0.3.1 才补上 per-binary 签名与 `latest.json` 生成 —— 所以 v0.3.0 自身在后续运行中**还不会**弹出更新提示。

### 本地快速验证签名密钥对(在 4 分钟原生编译前发现问题)

如果想确认 GitHub 仓库里那两个 secret 是配对的,直接跑:

```bash
./scripts/check-signer.sh --key ~/path/to/nicessh.key --password ~/path/to/nicessh.pwd
# 或模拟 CI 的环境变量方式:
export TAURI_SIGNING_PRIVATE_KEY=... TAURI_SIGNING_PRIVATE_KEY_PASSWORD=...
./scripts/check-signer.sh --from-env
```

退出码 0 = 配对正确,2 = 密码错,3 = key 格式错(常见于从 GitHub secret UI 复制后残留 CRLF)。

### 注意事项(1.0 之前)

- **macOS**: 自动更新后的安装包在首次打开时仍会触发 Gatekeeper。和手动下载的处理方式相同:右键 → 打开 → 「打开」。公证是 v1.0.0 的工作。
- **Windows**: 自动更新后的安装包在首次打开时仍会触发 SmartScreen。和手动下载的处理方式相同:「更多信息」→ 「仍要运行」。EV 代码签名是 v1.0.0 的工作。
- **Linux `.deb` / `.rpm`**: 升级需要 `sudo`(底层调用系统包管理器)。Linux 上推荐用 `.AppImage` 渠道做自动更新 —— 它不需要 root 即可就地升级。
- **手动重启**: 下载完成后,NiceSSH 会提示你关闭并重新打开应用以应用更新。一键进程内重启(`@tauri-apps/plugin-process`)计划在后续版本中提供;在此之前,手动关闭再打开的体验完全一致。

## 当前限制(MVP)

- **无内置终端。** NiceSSH 不替代你的 shell,只是它的补充:在 GUI 里编辑身份 / 项目,然后照常用命令行执行 `git`。
- **无 commit / push / pull 界面。** 应用只读展示最近 10 次提交,用于「push 之前确认这就是正确的身份」。真正的 push 仍然在你的终端完成。
- **0.1.0 没有代码签名 / 公证。** macOS Gatekeeper 和 Windows SmartScreen 会显示警告。v1.0.0 计划见 [CHANGELOG.md](CHANGELOG.md)。

## 许可证

[MIT](LICENSE) — 全文见 LICENSE 文件。
