mod auth;
mod crypto;
mod db;
mod errors;
mod models;
mod routes;

use crate::auth::{hash_password, verify_password, AppState};
use crate::crypto::MasterKey;
use crate::db::init_db;
use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use base64::Engine;
use clap::Parser;
use rust_embed::Embed;
use std::io::{self, Write};
use zeroize::Zeroize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

#[derive(Embed)]
#[folder = "static/"]
struct Assets;

#[derive(Parser)]
#[command(name = "collpasswd", version, about = "🔐 协同密码管理工具")]
struct Cli {
    /// 监听地址 (例如 0.0.0.0:443)
    #[arg(short, long, default_value = "0.0.0.0:443")]
    addr: String,

    /// 数据库文件路径
    #[arg(short, long, default_value = "collpasswd.db")]
    db: String,

    /// 不使用 TLS (HTTP 明文)
    #[arg(long)]
    no_tls: bool,
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/').to_string();
    let path = if path.is_empty() { "index.html".into() } else { path };

    match Assets::get(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let body = Body::from(file.data);
            ([(header::CONTENT_TYPE, mime.as_ref())], body).into_response()
        }
        None => {
            if !path.starts_with("api/") {
                if let Some(idx) = Assets::get("index.html") {
                    return (
                        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        Body::from(idx.data),
                    )
                        .into_response();
                }
            }
            (StatusCode::NOT_FOUND, "Not found").into_response()
        }
    }
}

// ─── Secure password input ────────────────────────────────────────────────

#[cfg(unix)]
fn read_password_masked(prompt: &str) -> String {
    use std::os::fd::AsRawFd;

    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();

    eprint!("{}", prompt);
    io::stdout().flush().ok();

    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    let mut pw = String::new();

    unsafe {
        libc::tcgetattr(fd, termios.as_mut_ptr());
        let mut new = termios.assume_init();
        let old = new;

        new.c_lflag &= !(libc::ECHO | libc::ICANON);
        new.c_cc[libc::VMIN] = 1;
        new.c_cc[libc::VTIME] = 0;
        libc::tcsetattr(fd, libc::TCSANOW, &new);

        let mut buf = [0u8; 1];
        loop {
            if libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, 1) <= 0 {
                break;
            }
            match buf[0] {
                b'\n' | b'\r' => break,
                0x7f | 0x08 => {
                    pw.pop();
                    eprint!("\x08 \x08");
                }
                0x03 => {
                    libc::tcsetattr(fd, libc::TCSANOW, &old);
                    std::process::exit(1);
                }
                c => {
                    pw.push(c as char);
                    eprint!("*");
                }
            }
            io::stdout().flush().ok();
        }
        libc::tcsetattr(fd, libc::TCSANOW, &old);
    }

    eprintln!();
    pw
}

#[cfg(not(unix))]
fn read_password_masked(prompt: &str) -> String {
    eprint!("{}", prompt);
    io::stdout().flush().ok();
    let mut pw = String::new();
    io::stdin().read_line(&mut pw).ok();
    println!();
    pw.trim().to_string()
}

// ─── TLS setup ─────────────────────────────────────────────────────────────

/// Generate a self-signed cert in memory (valid 30 years), return a TLS config.
/// Every restart gets a fresh cert — no files, no rebuilds needed.
async fn build_tls_config() -> RustlsConfig {
    tracing::info!("Generating self-signed TLS certificate...");

    use rcgen::{CertificateParams, DistinguishedName, DnType, IsCa, BasicConstraints};

    let mut params = CertificateParams::new(vec!["collpasswd".into(), "localhost".into()]);
    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(365 * 30);

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "CollPasswd");
    dn.push(DnType::OrganizationName, "Self-Hosted");
    params.distinguished_name = dn;

    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    let cert = rcgen::Certificate::from_params(params).expect("create cert");

    let cert_pem = cert.serialize_pem().expect("serialize cert");
    let key_pem = cert.serialize_private_key_pem();

    RustlsConfig::from_pem(cert_pem.into_bytes(), key_pem.into_bytes())
        .await
        .expect("failed to build TLS config")
}

// ─── DB init flow ──────────────────────────────────────────────────────────

fn resolve_master_key(conn: &rusqlite::Connection, password: &str) -> MasterKey {
    let salt_b64: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'master_key_salt'",
            [],
            |row| row.get(0),
        )
        .expect("salt not found");

    let salt = base64::engine::general_purpose::STANDARD
        .decode(&salt_b64)
        .expect("invalid salt encoding");

    let mut salt_arr = [0u8; 16];
    salt_arr.copy_from_slice(&salt[..16]);

    MasterKey::derive_from_password(password, &salt_arr)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let db_exists = Path::new(&cli.db).exists();
    let pool = init_db(Path::new(&cli.db))?;
    tracing::info!("Database: {}", cli.db);

    let (master_key, jwt_secret_str) = {
        let conn = pool.get().expect("db connect");
        let mut pw: String;

        if !db_exists {
            pw = loop {
                let p1 = read_password_masked("🔐 设置管理员密码: ");
                if p1.len() < 6 {
                    println!("密码至少需要 6 位，请重新输入");
                    continue;
                }
                let p2 = read_password_masked("🔐 再次输入: ");
                if p1 == p2 {
                    break p1;
                }
                println!("两次输入不一致，请重新设置");
            };

            let salt = MasterKey::generate_salt();
            let salt_b64 = base64::engine::general_purpose::STANDARD.encode(salt);
            conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES ('master_key_salt', ?1)",
                [&salt_b64],
            )?;
            let hash = hash_password(&pw)?;
            conn.execute(
                "INSERT INTO users (username, password_hash, role) VALUES ('admin', ?1, 'admin')",
                [&hash],
            )?;
            println!("✅ 管理员账号已创建");
            println!("⚠️  请牢记此密码！忘记密码将导致所有数据永久丢失。");

            let mk = MasterKey::derive_from_password(&pw, &salt);
            let jwt = uuid::Uuid::new_v4().to_string();
            pw.zeroize();
            (mk, jwt)
        } else {
            pw = read_password_masked("🔐 管理员密码: ");

            let admin_hash: String = conn
                .query_row(
                    "SELECT password_hash FROM users WHERE role = 'admin' LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| anyhow::anyhow!("数据库中没有管理员账号"))?;

            let ok = verify_password(&pw, &admin_hash)
                .map_err(|e| anyhow::anyhow!("验证失败: {}", e))?;
            if !ok {
                pw.zeroize();
                anyhow::bail!("❌ 密码错误");
            }
            println!("✅ 验证通过");

            let mk = resolve_master_key(&conn, &pw);
            let jwt = uuid::Uuid::new_v4().to_string();
            pw.zeroize();
            (mk, jwt)
        }
    };

    let jwt_secret = Arc::new(auth::JwtSecret(jwt_secret_str));

    // Read JWT expiry from config (default 24h)
    let jwt_expiry_hours = {
        let conn = pool.get().expect("db connect");
        conn.query_row(
            "SELECT value FROM config WHERE key = 'jwt_expiry_hours'",
            [],
            |row| row.get::<_, String>(0),
        ).ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(24)
    };
    let jwt_expiry_hours = Arc::new(AtomicI64::new(jwt_expiry_hours));

    let state = Arc::new(AppState {
        db: pool,
        jwt_secret,
        master_key,
        jwt_expiry_hours,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/auth/login", post(routes::auth_routes::login))
        .route("/api/auth/admin-login", post(routes::auth_routes::admin_login))
        .route("/api/auth/register", post(routes::auth_routes::register))
        .route("/api/auth/me", get(routes::auth_routes::me))
        .route("/api/users", get(routes::admin_routes::list_users_brief))
        .route("/api/admin/users", get(routes::admin_routes::list_users).post(routes::admin_routes::create_user))
        .route("/api/admin/users/:id", delete(routes::admin_routes::delete_user))
        .route("/api/books", get(routes::book_routes::list_books).post(routes::book_routes::create_book))
        .route("/api/books/:id", get(routes::book_routes::get_book).put(routes::book_routes::update_book).delete(routes::book_routes::delete_book))
        .route("/api/books/:id/members", get(routes::book_routes::list_members).post(routes::book_routes::add_member))
        .route("/api/books/:id/members/:uid", put(routes::book_routes::update_member_role).delete(routes::book_routes::remove_member))
        .route("/api/books/:id/sheets", get(routes::sheet_routes::list_sheets).post(routes::sheet_routes::create_sheet))
        .route("/api/sheets/:id", put(routes::sheet_routes::update_sheet).delete(routes::sheet_routes::delete_sheet))
        .route("/api/sheets/:id/passwords", get(routes::password_routes::list_passwords).post(routes::password_routes::create_password))
        .route("/api/sheets/:sid/passwords/:pid", get(routes::password_routes::get_password))
        .route("/api/passwords/:id", put(routes::password_routes::update_password).delete(routes::password_routes::delete_password))
        .route("/api/audit", get(routes::audit_routes::list_audit_global))
        .route("/api/books/:id/audit", get(routes::audit_routes::list_audit_book))
        .route("/api/books/:id/search", get(routes::book_routes::search_book))
        .route("/api/admin/settings", get(routes::admin_routes::get_settings).put(routes::admin_routes::update_settings))
        .route("/api/sheets/:id/export", get(routes::io_routes::export_sheet))
        .route("/api/books/:id/export", get(routes::io_routes::export_book))
        .route("/api/sheets/:id/import", post(routes::io_routes::import_sheet))
        .route("/api/books/:id/import", post(routes::io_routes::import_book))
        .layer(cors)
        .with_state(state);

    let app = app.fallback(static_handler);

    if cli.no_tls {
        tracing::info!("Listening on http://{}", cli.addr);
        let listener = tokio::net::TcpListener::bind(&cli.addr).await?;
        axum::serve(listener, app).await?;
    } else {
        let tls_config = build_tls_config().await;
        let addr: std::net::SocketAddr = cli.addr.parse().expect("invalid address");
        tracing::info!("Listening on https://{}", addr);
        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await?;
    }

    Ok(())
}
