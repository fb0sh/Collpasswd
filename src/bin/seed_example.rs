// seed-example — 生成示例数据库并打印账户信息
//
// Usage:
//   cargo run --bin seed-example
//   cargo run --bin seed-example -- --db demo.db
//   cargo run --bin seed-example -- --admin-password MyAdminP@ss

use clap::Parser;
use rand::rngs::OsRng;
use rand::RngCore;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::path::Path;

use collpasswd::auth::hash_password;
use collpasswd::crypto::{EccCrypto, MasterKey};
use collpasswd::db::init_db;

#[derive(Parser)]
#[command(name = "seed-example", about = "生成示例数据并打印账户信息")]
struct Args {
    /// 数据库文件路径
    #[arg(short, long, default_value = "collpasswd.db")]
    db: String,

    /// 管理员密码（不指定则自动生成）
    #[arg(long)]
    admin_password: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let db_path = Path::new(&args.db);

    // 如果数据库已存在，先删除
    if db_path.exists() {
        std::fs::remove_file(db_path)?;
        let wal = format!("{}-wal", args.db);
        let shm = format!("{}-shm", args.db);
        let _ = std::fs::remove_file(&wal);
        let _ = std::fs::remove_file(&shm);
    }

    // 初始化数据库（创建表结构）
    let pool = init_db(db_path)?;
    let conn = pool.get()?;

    // ===== 生成管理员密码 =====
    let admin_password = match args.admin_password {
        Some(p) => p,
        None => {
            let mut buf = [0u8; 12];
            OsRng.fill_bytes(&mut buf);
            BASE64.encode(buf)
        }
    };

    // ===== 创建 MasterKey 和 salt =====
    let salt = MasterKey::generate_salt();
    let salt_b64 = BASE64.encode(salt);
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('master_key_salt', ?1)",
        [&salt_b64],
    )?;

    let master_key = MasterKey::derive_from_password(&admin_password, &salt);

    // ===== 创建用户 =====
    // 管理员
    let admin_hash = hash_password(&admin_password)?;
    conn.execute(
        "INSERT INTO users (username, password_hash, role) VALUES ('admin', ?1, 'admin')",
        [&admin_hash],
    )?;

    // alice
    let alice_hash = hash_password("alice123")?;
    conn.execute(
        "INSERT INTO users (username, password_hash, role) VALUES ('alice', ?1, 'user')",
        [&alice_hash],
    )?;

    // bob
    let bob_hash = hash_password("bob123")?;
    conn.execute(
        "INSERT INTO users (username, password_hash, role) VALUES ('bob', ?1, 'user')",
        [&bob_hash],
    )?;

    // ===== 生成 ECC 密钥对 =====
    let (sk, pk) = EccCrypto::generate_keypair()?;
    let sk_pem = EccCrypto::secret_key_to_pem(&sk)?;
    let pk_pem = EccCrypto::public_key_to_pem(&pk)?;

    let sk_encrypted = master_key.encrypt_private_key(&sk_pem)?;

    // ===== 创建 Book =====
    conn.execute(
        "INSERT INTO books (name, description, ecc_private_key, ecc_public_key, created_by)
         VALUES ('团队共享密码库', '团队共享密码库，包含服务器、WiFi 等密码', ?1, ?2, 1)",
        rusqlite::params![sk_encrypted, pk_pem],
    )?;
    let book_id = conn.last_insert_rowid();

    // 创建者自动加入成员
    conn.execute(
        "INSERT INTO book_members (book_id, user_id, role) VALUES (?1, 1, 'edit')",
        [book_id],
    )?;
    // alice 可编辑
    conn.execute(
        "INSERT INTO book_members (book_id, user_id, role) VALUES (?1, 2, 'edit')",
        [book_id],
    )?;
    // bob 仅查看
    conn.execute(
        "INSERT INTO book_members (book_id, user_id, role) VALUES (?1, 3, 'view')",
        [book_id],
    )?;

    // ===== 创建 Sheet: 服务器密码 =====
    conn.execute(
        "INSERT INTO sheets (book_id, name, description) VALUES (?1, '服务器密码', '生产/测试/开发服务器')",
        [book_id],
    )?;
    let sheet1_id = conn.last_insert_rowid();

    // ===== 添加密码：服务器密码 =====
    let passwords_server = vec![
        ("生产数据库", "dbadmin", "P@ssw0rd_prod_2024", "db01.example.com", "MySQL 主库"),
        ("测试服务器 SSH", "root", "test_server_ssh!", "10.0.1.100", "内网测试机"),
        ("开发服务器 SSH", "developer", "dev_pass_123", "10.0.1.200", "开发环境"),
    ];

    for (title, username, password, url, notes) in &passwords_server {
        let encrypted = EccCrypto::encrypt(&pk, password.as_bytes())?;
        conn.execute(
            "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes, updated_by_user_id, updated_by_username, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 'admin', datetime('now', '+8 hours'))",
            rusqlite::params![sheet1_id, title, username, encrypted, url, notes],
        )?;
    }

    // ===== 创建 Sheet: WiFi 密码 =====
    conn.execute(
        "INSERT INTO sheets (book_id, name, description) VALUES (?1, 'WiFi 密码', '办公室 WiFi')",
        [book_id],
    )?;
    let sheet2_id = conn.last_insert_rowid();

    let passwords_wifi = vec![
        ("办公区 2.4G", "office-wifi", "welcome2024!", "", "2.4GHz 访客网络"),
        ("办公区 5G", "office-wifi-5g", "welcome2024!", "", "内部使用"),
        ("会议室 WiFi", "meeting", "meeting#2024", "", "会议室专用"),
    ];

    for (title, username, password, url, notes) in &passwords_wifi {
        let encrypted = EccCrypto::encrypt(&pk, password.as_bytes())?;
        conn.execute(
            "INSERT INTO passwords (sheet_id, title, username, encrypted_password, url, notes, updated_by_user_id, updated_by_username, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 2, 'alice', datetime('now', '+8 hours'))",
            rusqlite::params![sheet2_id, title, username, encrypted, url, notes],
        )?;
    }

    // ===== 打印账户信息 =====
    println!();
    println!("═══════════════════════════════════════════");
    println!("  🎉 示例数据库已生成");
    println!("  文件: {}", args.db);
    println!("═══════════════════════════════════════════");
    println!();
    println!("  👑 管理员账号");
    println!("     用户名: admin");
    println!("     密码:   {}", admin_password);
    println!();
    println!("  👤 测试用户");
    println!("     用户名: alice");
    println!("     密码:   alice123");
    println!("     权限:   团队共享密码库 → 可编辑");
    println!();
    println!("     用户名: bob");
    println!("     密码:   bob123");
    println!("     权限:   团队共享密码库 → 仅查看");
    println!();
    println!("  📚 项目: 团队共享密码库");
    println!("     ├─ 服务器密码 (7条)");
    println!("     └─ WiFi 密码 (3条)");
    println!();
    println!("  ▶ 启动服务: cargo run -- --no-tls --addr 0.0.0.0:8080 --db {}", args.db);
    println!();
    println!("  ⚠️  请及时修改默认密码！");
    println!("═══════════════════════════════════════════");
    println!();

    Ok(())
}
