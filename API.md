# CollPasswd API 文档

## 基础信息

- **Base URL**: `https://your-host:443`
- **协议**: HTTPS (默认) / HTTP (需 `--no-tls`)
- **所有响应格式**: `{ success: bool, data?: T, error?: string }`
- **认证方式**: Bearer Token（JWT，24h 有效期，可在平台设置中调整）

---

## 目录

1. [认证](#1-认证)
2. [用户管理](#2-用户管理admin-专属)
3. [项目 (Book)](#3-项目-book)
4. [成员管理](#4-成员管理)
5. [密码表 (Sheet)](#5-密码表-sheet)
6. [密码 (Password)](#6-密码-password)
7. [搜索](#7-搜索)
8. [导入导出](#8-导入导出)
9. [审计日志](#9-审计日志)
10. [平台设置](#10-平台设置)
11. [附录：权限速查表](#附录权限速查表)

---

## 1. 认证

### POST /api/auth/login

用户登录。

**Request Body:**
```json
{
  "username": "string",
  "password": "string"
}
```

**Response 200:**
```json
{
  "token": "jwt_token_string",
  "user": {
    "id": 1,
    "username": "alice",
    "role": "user"
  }
}
```

**Error 401:** `{"success": false, "error": "Invalid username or password"}`

---

### POST /api/auth/register

自助注册。强制创建普通用户 (`role: "user"`)，无法通过此接口创建管理员。

**Request Body:**
```json
{
  "username": "string",
  "password": "string"
}
```

**注意:** 请求中 `role` 字段会被忽略，始终创建 `role: "user"`。

**Response 200:**
```json
{
  "token": "jwt_token_string",
  "user": {
    "id": 2,
    "username": "bob",
    "role": "user"
  }
}
```

**Error 400:** `{"success": false, "error": "Username already exists"}`

---

### POST /api/auth/admin-login

管理员登录（仅用于内置 admin 账号，用户名固定为 `admin`）。

**Request Body:**
```json
{
  "username": "admin",
  "password": "string"
}
```

**Response 200:** 同上，`role: "admin"`。

---

### GET /api/auth/me

获取当前登录用户的信息。需要认证。

**Headers:**
```
Authorization: Bearer <token>
```

**Response 200:**
```json
{
  "id": 1,
  "username": "alice",
  "role": "user"
}
```

---

## 2. 用户管理（admin 专属）

### GET /api/admin/users

获取所有用户列表。

**需要:** admin 权限

**Response 200:**
```json
[
  {
    "id": 1,
    "username": "admin",
    "role": "admin",
    "created_at": "2024-01-01 00:00:00"
  },
  {
    "id": 2,
    "username": "alice",
    "role": "user",
    "created_at": "2024-01-02 12:00:00"
  }
]
```

---

### POST /api/admin/users

创建用户。强制创建普通用户，无法创建管理员。

**需要:** admin 权限

**Request Body:**
```json
{
  "username": "charlie",
  "password": "securepass123"
}
```

**注意:** `role` 字段会被忽略，始终创建 `role: "user"`。

---

### DELETE /api/admin/users/:id

删除用户。

**需要:** admin 权限

**限制:**
- 不可删除自己（返回 400）
- 不可删除内置 `admin` 账号（返回 403）

**Response 200:** `{"success": true}`

---

### GET /api/users

获取所有用户的简要列表（仅 `id` 和 `username`，用于成员选择器下拉框）。

**需要:** 任何已认证用户

**Response 200:**
```json
[
  {"id": 1, "username": "admin"},
  {"id": 2, "username": "alice"}
]
```

---

## 3. 项目 (Book)

### GET /api/books

获取当前用户可访问的项目列表。

**需要:** 认证

- 普通用户：仅看到自己参与的项目（作为成员或持有者）
- 管理员：看到所有项目

**Response 200:**
```json
[
  {
    "id": 1,
    "name": "团队密码库",
    "description": "公司内部服务密码",
    "created_at": "2024-01-01 00:00:00",
    "created_by": 1,
    "member_count": 5,
    "is_holder": true
  }
]
```

---

### POST /api/books

创建新项目。任何用户都可创建。

**需要:** 认证

**Request Body:**
```json
{
  "name": "新项目",
  "description": "项目描述（可选）"
}
```

创建者自动成为项目的**持有者 (Holder)**，并自动加入成员列表（角色 `edit`）。

---

### GET /api/books/:id

获取项目详情。

**需要:** 项目成员或管理员

**Response 200:**
```json
{
  "id": 1,
  "name": "团队密码库",
  "description": "...",
  "created_at": "...",
  "created_by": 1,
  "member_count": 5,
  "is_holder": false
}
```

---

### PUT /api/books/:id

更新项目名称/描述。

**需要:** 项目持有者或管理员

**Request Body:**
```json
{
  "name": "新名称（可选）",
  "description": "新描述（可选）"
}
```

---

### DELETE /api/books/:id

删除项目（级联删除所有密码表和密码）。

**需要:** 项目持有者或管理员

**Response 200:** `{"success": true}`

---

## 4. 成员管理

### GET /api/books/:id/members

获取项目成员列表。

**需要:** 项目成员

**Response 200:**
```json
[
  {
    "id": 1,
    "user_id": 1,
    "username": "alice",
    "role": "edit"
  },
  {
    "id": 2,
    "user_id": 2,
    "username": "bob",
    "role": "view"
  }
]
```

**角色说明:**
| role | 说明 |
|---|---|
| `edit` | 可编辑 — 查看 + 添加/修改/删除密码和表 |
| `view` | 仅查看 — 只能查看密码 |

---

### POST /api/books/:id/members

添加成员到项目。

**需要:** 项目持有者或管理员

**Request Body:**
```json
{
  "username": "alice",
  "role": "edit"
}
```

**角色:** 只接受 `edit` 或 `view`。

**Error 400:** `{"success": false, "error": "User is already a member of this book"}`

---

### PUT /api/books/:id/members/:uid

修改成员的角色。

**需要:** 项目持有者或管理员

**限制:** 不可修改项目持有者的角色。

**Request Body:**
```json
{
  "role": "view"
}
```

---

### DELETE /api/books/:id/members/:uid

从项目中移除成员。

**需要:** 项目持有者或管理员

**限制:** 不可移除项目持有者。

**Response 200:** `{"success": true}`

---

## 5. 密码表 (Sheet)

### GET /api/books/:id/sheets

获取项目下的密码表列表。

**需要:** 项目成员

**Response 200:**
```json
[
  {
    "id": 1,
    "book_id": 1,
    "name": "服务器密码",
    "description": "各环境服务器 SSH 密码",
    "created_at": "...",
    "password_count": 12
  }
]
```

---

### POST /api/books/:id/sheets

创建密码表。

**需要:** 可编辑权限

**Request Body:**
```json
{
  "name": "数据库密码",
  "description": "数据库相关（可选）"
}
```

---

### PUT /api/sheets/:id

更新密码表。

**需要:** 可编辑权限

**Request Body:**
```json
{
  "name": "新名称（可选）",
  "description": "新描述（可选）"
}
```

---

### DELETE /api/sheets/:id

删除密码表（级联删除所有密码）。

**需要:** 可编辑权限

**Response 200:** `{"success": true}`

---

## 6. 密码 (Password)

### GET /api/sheets/:id/passwords

获取密码表中的密码列表（仅元数据，**不包含密码明文**）。

**需要:** 项目成员

**Response 200:**
```json
[
  {
    "id": 1,
    "sheet_id": 1,
    "title": "生产服务器 SSH",
    "username": "root",
    "url": "192.168.1.1",
    "notes": "root 用户",
    "updated_at": "2024-03-15T14:30:00",
    "updated_by_username": "alice",
    "has_password": true
  }
]
```

**安全说明:** `has_password` 始终为 `true`，仅表示该条目存在加密密码。密码明文不会出现在列表响应中。

---

### POST /api/sheets/:id/passwords

创建密码条目。

**需要:** 可编辑权限

**Request Body:**
```json
{
  "title": "生产服务器 SSH",
  "username": "root",
  "password": "s3cr3t!",
  "url": "192.168.1.1",
  "notes": "root 用户"
}
```

所有字段中，仅 `title` 和 `password` 为必填。创建时自动设置 `updated_by` 为当前用户。

---

### GET /api/sheets/:id/passwords/:pid

获取密码详情（解密后返回明文密码）。

**需要:** 项目成员

**Response 200:**
```json
{
  "id": 1,
  "sheet_id": 1,
  "title": "生产服务器 SSH",
  "username": "root",
  "password": "s3cr3t!",
  "url": "192.168.1.1",
  "notes": "root 用户",
  "updated_at": "2024-03-15T14:30:00",
  "updated_by_username": "alice"
}
```

---

### PUT /api/passwords/:id

更新密码条目。

**需要:** 可编辑权限

**Request Body:**
```json
{
  "title": "新标题（可选）",
  "username": "新用户名（可选）",
  "password": "新密码（可选）",
  "url": "新网址（可选）",
  "notes": "新备注（可选）"
}
```

更新时自动设置 `updated_at = datetime('now')` 和 `updated_by` 为当前用户。

---

### DELETE /api/passwords/:id

删除密码条目。

**需要:** 可编辑权限

**Response 200:** `{"success": true}`

---

## 7. 搜索

### GET /api/books/:id/search?q=keyword

搜索项目内所有密码表中的密码（支持 `title`、`username`、`url`、`notes` 字段的 `LIKE` 匹配）。

**需要:** 项目成员

**参数:**
| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `q` | string | 是 | 搜索关键词 |

**Response 200:**
```json
[
  {
    "id": 1,
    "title": "生产服务器 SSH",
    "username": "root",
    "url": "192.168.1.1",
    "sheet_id": 1,
    "sheet_name": "服务器密码",
    "updated_at": "2024-03-15T14:30:00",
    "updated_by_username": "alice"
  }
]
```

**性能:** 后端单条 SQL `LIKE` 查询，`LIMIT 100`，结果集极小。无需客户端预加载。

---

## 8. 导入导出

所有导入导出格式均为 **CSV**（UTF-8 编码，支持带引号和逗号的字段）。

### GET /api/sheets/:id/export

导出单个密码表的所有密码（解密后明文）。

**需要:** 项目成员

**Response 200:** CSV 文件

```
title,username,password,url,notes
生产服务器 SSH,root,s3cr3t!,192.168.1.1,root 用户
测试服务器,admin,test123,10.0.0.1,开发环境
```

---

### GET /api/books/:id/export

导出整个项目的所有密码（按表分组）。

**需要:** 项目成员

**Response 200:** CSV 文件

```
sheet,title,username,password,url,notes
服务器密码,生产服务器 SSH,root,s3cr3t!,192.168.1.1,root 用户
服务器密码,测试服务器,admin,test123,10.0.0.1,开发环境
数据库密码,MySQL 生产,dbadmin,dbpass!,db01.example.com,主库
```

---

### POST /api/sheets/:id/import

导入 CSV 到指定密码表。

**需要:** 可编辑权限

**Request Body:** CSV 文件内容（`Content-Type: application/json`，body 为 JSON 数组）

```json
[
  {
    "title": "新服务器",
    "username": "admin",
    "password": "pass123",
    "url": "10.0.0.2",
    "notes": ""
  }
]
```

**Response 200:**
```json
{
  "success": true,
  "imported": 10,
  "errors": 0
}
```

**注意:** 导入时自动设置 `updated_by` 为当前用户，时间为导入时间。

---

### POST /api/books/:id/import

导入 CSV（含 `sheet` 列）到项目，按 `sheet` 列自动匹配/创建密码表。

**需要:** 可编辑权限

**Request Body:** JSON 数组

```json
[
  {
    "sheet_name": "服务器密码",
    "passwords": [
      {"title": "新服务器", "username": "admin", "password": "pass123"}
    ]
  }
]
```

- 如果项目下已存在同名表，则追加到该表
- 如果不存在，则自动创建

**Response 200:**
```json
{
  "success": true,
  "imported": 10,
  "errors": 0
}
```

---

## 9. 审计日志

### GET /api/audit?limit=50&offset=0&action=view_password

全局审计日志。

**需要:** admin 权限

**参数:**
| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `limit` | int | 50 | 每页条数（最大 500） |
| `offset` | int | 0 | 偏移量 |
| `action` | string | - | 过滤操作类型 |

**操作类型:**
| action | 说明 |
|---|---|
| `view_password` | 查看密码 |
| `create_password` | 创建密码 |
| `update_password` | 修改密码 |
| `delete_password` | 删除密码 |

**Response 200:**
```json
{
  "records": [
    {
      "id": 100,
      "book_id": 1,
      "book_name": "团队密码库",
      "sheet_id": 1,
      "password_id": 5,
      "user_id": 2,
      "username": "alice",
      "action": "view_password",
      "detail": "生产服务器 SSH：root",
      "created_at": "2024-03-15T14:30:00"
    }
  ],
  "total": 500,
  "limit": 50,
  "offset": 0
}
```

---

### GET /api/books/:id/audit?limit=20&offset=0&action=create_password

项目级审计日志。

**需要:** 项目成员

**参数 & 响应:** 同全局审计，但仅返回指定项目的记录。

---

## 10. 平台设置

### GET /api/admin/settings

获取平台设置。

**需要:** admin 权限

**Response 200:**
```json
{
  "jwt_expiry_hours": 24
}
```

---

### PUT /api/admin/settings

更新平台设置。

**需要:** admin 权限

**Request Body:**
```json
{
  "jwt_expiry_hours": 72
}
```

**约束:** `jwt_expiry_hours` 必须在 1–720 之间（1 小时 ~ 30 天）。

**生效方式:** 设置即时写入 `config` 表并更新内存值。现有登录不受影响，**新的登录**使用新有效期。

**Response 200:**
```json
{
  "success": true,
  "jwt_expiry_hours": 72
}
```

---

## 附录：权限速查表

### 全局权限

| 资源 | 操作 | user | admin |
|---|---|---|---|
| 用户管理 | 列出/创建/删除用户 | — | ✅ |
| 平台设置 | 查看/修改 | — | ✅ |
| 全局审计 | 查看 | — | ✅ |
| 所有项目 | 看到所有项目 | — | ✅ |

### 项目内权限

| 资源 | 操作 | 持有者 (holder) | 可编辑 (edit) | 仅查看 (view) |
|---|---|---|---|---|
| 项目 | 查看详情 | ✅ | ✅ | ✅ |
| 项目 | 编辑名称/描述 | ✅ | — | — |
| 项目 | 删除 | ✅ | — | — |
| 成员 | 查看列表 | ✅ | ✅ | ✅ |
| 成员 | 添加/移除 | ✅ (不可移除自己) | — | — |
| 成员 | 修改角色 | ✅ (不可改持有者) | — | — |
| 密码表 | 查看列表 | ✅ | ✅ | ✅ |
| 密码表 | 创建/编辑/删除 | ✅ | ✅ | — |
| 密码 | 查看列表（元数据） | ✅ | ✅ | ✅ |
| 密码 | 查看明文 | ✅ | ✅ | ✅ |
| 密码 | 创建/编辑/删除 | ✅ | ✅ | — |
| 搜索 | 跨表搜索 | ✅ | ✅ | ✅ |
| 导出 | CSV 导出 | ✅ | ✅ | ✅ |
| 导入 | CSV 导入 | ✅ | ✅ | — |
| 审计 | 项目审计日志 | ✅ | ✅ | ✅ |

### 安全限制

| 规则 | 实现 |
|---|---|
| 任何用户都可创建项目 | `POST /api/books` 无需 admin |
| 项目持有者不可被移出 | `DELETE /api/books/:id/members/:uid` 返回 403 |
| 项目持有者角色不可被更改 | `PUT /api/books/:id/members/:uid` 返回 403 |
| 内置 admin 不可被删除 | `DELETE /api/admin/users/:id` 返回 403 |
| 内置 admin 角色不可更改 | 后端强制忽略 `role` 字段 |
| 自助注册只能创建普通用户 | `POST /api/auth/register` 强制 `role: "user"` |
| 管理员创建用户也只能创建普通用户 | `POST /api/admin/users` 强制 `role: "user"` |
