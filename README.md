# CollPasswd — 协同密码管理

密码共享，本该如此安全。

[![Release](https://github.com/fb0sh/Collpasswd/actions/workflows/release.yml/badge.svg)](https://github.com/fb0sh/Collpasswd/actions/workflows/release.yml)

---

## 设计理念

CollPasswd 的设计围绕三个核心原则：

### 1. 最小权限，零信任

系统预设零信任：任何用户注册后看不到任何数据，直到被项目所有者明确授权。项目内权限仅分三级——所有者、可编辑、仅查看。不存在"全局管理员可以看所有密码"的设计（仅系统 admin 可管理用户和审计，但密码的访问权完全由项目所有者控制）。

### 2. 服务端不掌握明文

密码的加解密全部在服务端进行，但解密密钥（每本 Book 的 ECC 私钥）本身由管理员密码派生的 MasterKey 加密存储。MasterKey **永不落盘**，每次服务器重启都必须由管理员手动输入密码才能恢复。这意味着：
- 即使数据库被窃取，攻击者也无法解密任何密码
- 即使服务器被攻陷，内存中同时存在明文密码的唯一时机是某位用户刚好正在查看某条密码的短暂瞬间

### 3. 极简部署，默认安全

单二进制 + SQLite + 自签名 TLS，无需配置数据库、反向代理、证书文件。启动即用，安全默认启用。

---

## 特色

- **端到端加密** — ECC P-256 (ECDH + HKDF-SHA256 + AES-256-GCM)，服务端不掌握明文
- **单二进制部署** — 前端 SPA 编译进二进制文件，零外部依赖
- **内建 TLS** — 启动时自动生成自签名证书，无需额外配置
- **多用户协作** — 以"项目(Book)"为单位共享密码，支持细粒度权限
- **审计日志** — 记录所有密码查看/添加/修改/删除操作
- **导入导出** — 项目和表级别的 CSV 导入/导出，支持 Excel 兼容的 UTF-8 BOM
- **导入预览** — CSV 导入前预览数据，确认无误后再写入
- **导入错误详情** — 失败逐条显示原因（标题为空/密码为空/加密失败等）
- **下载导入模板** — 一键下载 CSV 模板，按模板填写即可快捷导入
- **批量选择删除** — 密码列表和搜索结果中支持多选 + 全选，一键批量删除
- **自助注册** — 用户可自行注册账号，由项目所有者分配权限
- **密码内存安全** — 密码仅在显示在界面时短暂存在于浏览器内存中，操作完成后立即清除

---

## 系统截图


<img width="1337" height="829" alt="image" src="https://github.com/user-attachments/assets/c23f31cf-0de9-4008-84b0-fe6baf6af695" />

<img width="1333" height="792" alt="image" src="https://github.com/user-attachments/assets/71c39862-aaed-42a8-b7a0-f83b0eed07f2" />

<img width="1325" height="794" alt="image" src="https://github.com/user-attachments/assets/55dd3f2a-ff7e-442a-9f57-a0794b2a91c1" />

<img width="1309" height="777" alt="image" src="https://github.com/user-attachments/assets/d74b7fdb-93aa-4c53-827b-a241db98036b" />

<img width="1318" height="777" alt="image" src="https://github.com/user-attachments/assets/788205aa-735d-47f1-98ec-e196b109be56" />

<img width="1185" height="707" alt="image" src="https://github.com/user-attachments/assets/a86a2ff6-0940-45fd-975f-89a93614543d" />

<img width="1327" height="681" alt="image" src="https://github.com/user-attachments/assets/bb5ce37c-f924-44c3-9874-f34c6d16a0e7" />

<img width="1309" height="728" alt="image" src="https://github.com/user-attachments/assets/94a78666-28ca-41fd-b9e1-60b458ef0fff" />

<img width="1298" height="803" alt="image" src="https://github.com/user-attachments/assets/505c1ab6-179b-42cb-9bed-c06d7d72c644" />



## 快速开始

### 下载 & 运行

#### 方式一：下载预编译二进制（推荐）

从 [Releases](https://github.com/fb0sh/Collpasswd/releases) 下载对应平台的二进制文件：

| 平台 | 架构 | 文件 |
|------|------|------|
| Linux | x86_64 | `collpasswd-*-x86_64-linux-gnu.tar.gz` |
| Linux | ARM64 | `collpasswd-*-aarch64-linux-gnu.tar.gz` |
| macOS | Apple Silicon | `collpasswd-*-aarch64-macos.tar.gz` |
| Windows | x86_64 | `collpasswd-*-x86_64-windows.exe` |
| Windows | ARM64 | `collpasswd-*-aarch64-windows.exe` |

解压后直接运行：

```bash
# Linux / macOS
./collpasswd

# 指定端口和数据库路径
./collpasswd --addr 0.0.0.0:8443 --db data.db

# 不使用 TLS（HTTP 明文，仅限内网）
./collpasswd --no-tls --addr 0.0.0.0:8080
```

> 首次启动会提示设置**管理员密码**（≥6位，终端输入不会回显），请妥善保管！

#### 方式二：源码构建

```bash
git clone https://github.com/yourname/collpasswd.git
cd collpasswd
cargo build --release
./target/release/collpasswd
```

首次启动会：
1. 提示设置**管理员密码**（≥6位，终端输入不会回显）
2. 自动创建 SQLite 数据库及表结构
3. 生成 ECC 密钥对并加密存储
4. 生成自签名 TLS 证书（有效期 30 年）
5. 启动 HTTPS 服务

> ⚠️ **管理员密码即主密钥**，忘记密码 = **所有数据永久丢失**。请妥善保管！

### 访问

打开浏览器访问 `https://localhost:443`（或你指定的地址）。  
自签名证书会有安全警告，点「继续前往」即可。

---

## 安全保障

```
┌─────────────────────────────────────────────────────────────────┐
│                        管理员输入密码                            │
│                              │                                  │
│              ┌───────────────┴───────────────┐                  │
│              │                               │                  │
│         Argon2id 哈希                    HKDF-SHA256             │
│              │                               │                  │
│              ▼                               ▼                  │
│       SQLite 存储哈希               MasterKey (仅内存)           │
│       (用于登录验证)                  Drop 时 zeroize            │
│                                            │                    │
│                                     ┌──────┴──────┐             │
│                                     │  AES-256-GCM │            │
│                                     │ 加密每本 Book │            │
│                                     │ 的 ECC 私钥  │            │
│                                     └──────┬──────┘             │
│                                            │                    │
│                                     ┌──────┴──────┐             │
│                                     │  Book 的公钥  │            │
│                                     │ ECIES 加密   │            │
│                                     │ 每条密码条目  │            │
│                                     └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
```

| 威胁 | 防御措施 |
|---|---|
| 数据库泄露 | 密码以 ECIES 密文存储，无 MasterKey 无法解密 |
| 服务端被控 | MasterKey 仅存内存，私钥使用后 Zeroize 清零 |
| 管理员密码丢失 | 无重置机制——彻底不可恢复，数据永久丢失 |
| JWT 泄露 | 密钥每次重启随机生成，最长有效期 24h |
| 浏览器内存泄露 | 密码不再通过 `_passwordDecrypted` 持久缓存。点「隐藏」立即 delete。复制密码每次实时请求，操作完即释放。关闭编辑弹窗时清空输入框。离开视图时清空所有缓存 |
| 传输层嗅探 | TLS 默认启用，30 年自签名证书 |
| 未授权访问 | 所有 API 端点验证 JWT + 项目级成员检查 + 操作级权限检查 |

---

## 权限模型

### 全局角色

| 角色 | 说明 |
|---|---|
| **`admin`** | 内置系统管理员（首次启动创建），管理所有用户/项目/审计日志 |
| **`user`** | 普通用户，可创建自己的项目、被邀请加入他人项目 |

### 项目内身份

| 身份 | 来源 | 权限 |
|---|---|---|
| **所有者 (Holder)** | 项目创建者 | 完全控制 — 管理成员、编辑/删除项目、增删改密码 |
| **可编辑 (edit)** | 所有者添加 | 查看 + 添加/修改/删除密码和表 |
| **仅查看 (view)** | 所有者添加 | 仅可查看密码 |

### 安全边界

- 任何用户注册后看不到任何数据，直到被加入项目
- 所有者不可被移出项目，角色不可被更改
- 内置 `admin` 账号不可被删除，也不可被更改角色
- 创建用户时强制 role = "user"，无法通过 API 创建管理员

---

## 使用流程

```
登录 → 我的项目列表 → 点击项目 → 密码表列表（含跨表搜索）→ 点击表 → 密码列表
```

### 密码列表

| 标题 | 用户名 | 网址 | 密码 | 操作 |
|---|---|---|---|---|
| 生产服务器 | root | 192.168.1.1 | `••••` [显示] [复制] | [编辑] [删除] |

- **显示/隐藏** — 一键解密，`••••` 切换为明文。隐藏后立即释放内存
- **复制** — 自动解密并复制到剪贴板，完成后立即释放
- **编辑** — 弹窗预填所有字段，关闭时清空密码输入框
- **删除** — 确认后删除

### 跨表搜索

在项目页面顶部搜索，即可搜索本项目**所有密码表**中的条目，结果也可直接查看和复制密码。

---

## API 概览

### 认证

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/auth/login` | 用户登录 |
| POST | `/api/auth/register` | 自助注册（仅创建普通用户） |
| POST | `/api/auth/admin-login` | 管理员登录 |
| GET | `/api/auth/me` | 当前用户信息 |

### 用户管理（admin 专属）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/admin/users` | 用户列表 |
| POST | `/api/admin/users` | 创建用户（仅创建普通用户） |
| DELETE | `/api/admin/users/:id` | 删除用户（内置 admin 不可删除） |
| GET | `/api/users` | 用户简要列表（用于成员选择器） |

### 项目 (Book)

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/books` | 我的项目列表（管理员看全部） |
| POST | `/api/books` | 创建项目 |
| GET | `/api/books/:id` | 项目详情 |
| PUT | `/api/books/:id` | 更新项目信息（所有者/admin） |
| DELETE | `/api/books/:id` | 删除项目（所有者/admin） |

### 成员管理

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/books/:id/members` | 成员列表 |
| POST | `/api/books/:id/members` | 添加成员（所有者/admin） |
| PUT | `/api/books/:id/members/:uid` | 修改成员角色（所有者/admin） |
| DELETE | `/api/books/:id/members/:uid` | 移除成员（所有者/admin，不可移除所有者） |

### 密码表 (Sheet)

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/books/:id/sheets` | 表列表 |
| POST | `/api/books/:id/sheets` | 创建表（需编辑权限） |
| PUT | `/api/sheets/:id` | 更新表（需编辑权限） |
| DELETE | `/api/sheets/:id` | 删除表（需编辑权限） |

### 密码 (Password)

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/sheets/:id/passwords` | 密码列表（仅元数据，不含明文） |
| POST | `/api/sheets/:id/passwords` | 创建密码（需编辑权限） |
| GET | `/api/sheets/:id/passwords/:pid` | 查看密码（解密后返回） |
| PUT | `/api/passwords/:id` | 更新密码（需编辑权限） |
| DELETE | `/api/passwords/:id` | 删除密码（需编辑权限） |

### 导入导出

| 方法 | 路径 | 格式 | 说明 |
|---|---|---|---|
| GET | `/api/sheets/:id/export` | CSV | 导出表（解密后明文）：`title,username,password,url,notes` |
| GET | `/api/books/:id/export` | CSV | 导出项目：`sheet,title,username,password,url,notes` |
| POST | `/api/sheets/:id/import` | CSV | 导入到表 |
| POST | `/api/books/:id/import` | CSV | 导入到项目（按 sheet 列自动匹配/创建表） |

### 审计

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/audit` | 全局审计日志（admin） |
| GET | `/api/books/:id/audit` | 项目审计日志 |

---

## 技术栈

| 层 | 技术 |
|---|---|
| 后端 | Rust + Axum |
| 数据库 | SQLite (r2d2 连接池) |
| 加密 | `p256` (ECDH) + `aes-gcm` + `hkdf` + `argon2` |
| 认证 | JWT (24h 有效期，密钥每次重启随机生成) |
| 前端 | 纯 JavaScript SPA（无框架） |
| 前端路由 | History API (pushState)，支持浏览器前进/后退/刷新 |

---

## CLI 参数

```
Usage: collpasswd [OPTIONS]

Options:
  -a, --addr <ADDR>      监听地址 [default: 0.0.0.0:443]
  -d, --db <DB>          数据库文件路径 [default: collpasswd.db]
      --no-tls           不使用 TLS（HTTP 明文）
  -h, --help             帮助信息
  -V, --version          版本信息
```

---

## 前端路由

SPA 使用 History API，URL 直观可读：

| 视图 | URL |
|---|---|
| 项目列表 | `/` |
| 项目详情 | `/book/:id` |
| 密码表 | `/book/:bookId/sheet/:sheetId` |
| 用户管理 | `/admin` |
| 审计日志 | `/audit` |

支持浏览器的前进/后退按钮和页面刷新。

---

## 开发

```bash
# 构建
cargo build

# 运行（开发模式）
RUST_LOG=debug cargo run -- --no-tls --addr 0.0.0.0:8080

# 测试
cargo test
```

---

## License

MIT
