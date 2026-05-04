use core::ptr;
use core::ffi::{c_char, c_void};
use ngx::ffi::{
    ngx_command_t, ngx_conf_t, ngx_http_module_t, ngx_int_t, ngx_module_t, ngx_str_t, ngx_uint_t,
    NGX_CONF_TAKE1, NGX_HTTP_LOC_CONF, NGX_HTTP_LOC_CONF_OFFSET, NGX_HTTP_MODULE, NGX_LOG_EMERG,
    ngx_http_request_t, ngx_http_variable_t, ngx_variable_value_t, ngx_http_add_variable,
    ngx_chain_t,
};
use ngx::http::{self, HttpModule, HttpModuleLocationConf, HttpRequestHandler, MergeConfigError, HTTPStatus};
use ngx::core::{Status, Buffer};
use ngx::{ngx_string, ngx_conf_log_error, ngx_log_debug_http, http_variable_get};
use core::ptr::NonNull;
use once_cell::sync::Lazy;
use moka::sync::Cache;
use sqlx::PgPool;
use prost::Message;
use tokio_stream::StreamExt;
use mikrom_proto::router::{AcmeChallengeUpdate, RouterConfigUpdate, TlsCertificateUpdate};

unsafe extern "C" {
    static ngx_process_slot: ngx::ffi::ngx_int_t;
}

mod crypto;

// Global shared state
pub static ROUTE_CACHE: Lazy<Cache<String, String>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(1000)
        .build()
});

pub static ACME_CACHE: Lazy<Cache<String, String>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(1000)
        .build()
});

static DB_POOL: Lazy<parking_lot::RwLock<Option<PgPool>>> = Lazy::new(|| {
    parking_lot::RwLock::new(None)
});

static TOKIO_HANDLE: Lazy<parking_lot::RwLock<Option<tokio::runtime::Handle>>> = Lazy::new(|| {
    parking_lot::RwLock::new(None)
});

#[cfg(test)]
pub fn set_test_db(pool: PgPool) {
    let mut guard = DB_POOL.write();
    *guard = Some(pool);
}

pub async fn wait_for_db_pool() {
    for _ in 0..10 {
        if DB_POOL.read().is_some() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

pub fn set_test_handle(handle: tokio::runtime::Handle) {
    let mut guard = TOKIO_HANDLE.write();
    *guard = Some(handle);
}

struct Module;

impl http::HttpModule for Module {
    fn module() -> &'static ngx_module_t {
        unsafe { &*::core::ptr::addr_of!(ngx_http_mikrom_router_module) }
    }

    unsafe extern "C" fn preconfiguration(cf: *mut ngx_conf_t) -> ngx_int_t {
        for mut v in unsafe { NGX_HTTP_MIKROM_ROUTER_VARS } {
            let var = NonNull::new(unsafe { ngx_http_add_variable(cf, &raw mut v.name, v.flags) });
            if var.is_none() {
                return Status::NGX_ERROR.into();
            }
            let mut var = var.unwrap();
            let var = unsafe { var.as_mut() };
            var.get_handler = v.get_handler;
            var.data = v.data;
        }
        Status::NGX_OK.into()
    }

    unsafe extern "C" fn postconfiguration(cf: *mut ngx_conf_t) -> ngx_int_t {
        let cf = unsafe { &mut *cf };
        http::add_phase_handler::<MikromRequestHandler>(cf)
            .map_or(Status::NGX_ERROR, |_| Status::NGX_OK)
            .into()
    }
}

static mut NGX_HTTP_MIKROM_ROUTER_VARS: [ngx_http_variable_t; 2] = [
    ngx_http_variable_t {
        name: ngx_string!("mikrom_target"),
        set_handler: None,
        get_handler: Some(ngx_http_mikrom_target_variable),
        data: 0,
        flags: 0,
        index: 0,
    },
    ngx_http_variable_t {
        name: ngx_string!("mikrom_acme_auth"),
        set_handler: None,
        get_handler: Some(ngx_http_mikrom_acme_auth_variable),
        data: 0,
        flags: 0,
        index: 0,
    },
];

fn get_host_header(request: &http::Request) -> Option<&str> {
    let r: &ngx_http_request_t = request.as_ref();
    let host = r.headers_in.host;
    if host.is_null() {
        return None;
    }
    
    let host_ngx = unsafe { &*host };
    let s = unsafe { core::slice::from_raw_parts(host_ngx.value.data, host_ngx.value.len) };
    core::str::from_utf8(s).ok()
}

http_variable_get!(
    ngx_http_mikrom_target_variable,
    |request: &mut http::Request, v: *mut ngx_variable_value_t, _: usize| {
        let host = match get_host_header(request) {
            Some(h) => h,
            None => return Status::NGX_DECLINED,
        };

        let target = match ROUTE_CACHE.get(host) {
            Some(cached) => cached,
            None => {
                return Status::NGX_DECLINED;
            }
        };

        let target_host = target.trim_start_matches("http://").trim_start_matches("https://");
        
        let v_ref = unsafe { NonNull::new(v).unwrap().as_mut() };
        let pool = request.pool();
        let val = pool.alloc_unaligned(target_host.len()).cast::<u8>();
        if val.is_null() {
            return Status::NGX_ERROR;
        }
        unsafe { ptr::copy_nonoverlapping(target_host.as_ptr(), val, target_host.len()) };

        v_ref.set_valid(1);
        v_ref.set_no_cacheable(0);
        v_ref.set_not_found(0);
        v_ref.set_len(target_host.len() as u32);
        v_ref.data = val;

        Status::NGX_OK
    }
);

http_variable_get!(
    ngx_http_mikrom_acme_auth_variable,
    |request: &mut http::Request, v: *mut ngx_variable_value_t, _: usize| {
        let uri = request.path().to_str().unwrap_or("");
        let token = match uri.rsplit_once('/') {
            Some((_, t)) => t,
            None => return Status::NGX_DECLINED,
        };

        let auth = match ACME_CACHE.get(token) {
            Some(a) => a,
            None => return Status::NGX_DECLINED,
        };

        let v_ref = unsafe { NonNull::new(v).unwrap().as_mut() };
        let pool = request.pool();
        let val = pool.alloc_unaligned(auth.len()).cast::<u8>();
        if val.is_null() {
            return Status::NGX_ERROR;
        }
        unsafe { ptr::copy_nonoverlapping(auth.as_ptr(), val, auth.len()) };

        v_ref.set_valid(1);
        v_ref.set_no_cacheable(0);
        v_ref.set_not_found(0);
        v_ref.set_len(auth.len() as u32);
        v_ref.data = val;

        Status::NGX_OK
    }
);

#[derive(Debug, Default)]
struct ModuleConfig {
    enable: bool,
}

unsafe impl HttpModuleLocationConf for Module {
    type LocationConf = ModuleConfig;
}

impl http::Merge for ModuleConfig {
    fn merge(&mut self, prev: &ModuleConfig) -> Result<(), MergeConfigError> {
        if prev.enable {
            self.enable = true;
        };
        Ok(())
    }
}

static mut NGX_HTTP_MIKROM_ROUTER_COMMANDS: [ngx_command_t; 3] = [
    ngx_command_t {
        name: ngx_string!("mikrom_router"),
        type_: (NGX_HTTP_LOC_CONF | NGX_CONF_TAKE1) as ngx_uint_t,
        set: Some(ngx_http_mikrom_router_commands_set_enable),
        conf: NGX_HTTP_LOC_CONF_OFFSET,
        offset: 0,
        post: ptr::null_mut(),
    },
    ngx_command_t {
        name: ngx_string!("mikrom_acme_challenge"),
        type_: (NGX_HTTP_LOC_CONF | NGX_CONF_TAKE1) as ngx_uint_t,
        set: Some(ngx_http_mikrom_router_commands_set_acme_challenge),
        conf: NGX_HTTP_LOC_CONF_OFFSET,
        offset: 0,
        post: ptr::null_mut(),
    },
    ngx_command_t::empty(),
];

struct MikromRequestHandler;

impl HttpRequestHandler for MikromRequestHandler {
    const PHASE: ngx::http::HttpPhase = ngx::http::HttpPhase::Access;
    type Output = Status;

    fn handler(request: &mut http::Request) -> Self::Output {
        let co = Module::location_conf(request).expect("module config is none");
        if !co.enable {
            return Status::NGX_DECLINED;
        }

        let host = get_host_header(request).unwrap_or("");
        if !host.is_empty() {
             ngx_log_debug_http!(request, "mikrom-router: processing request for host: {}", host);
        }

        Status::NGX_OK
    }
}

struct AcmeChallengeHandler;

impl HttpRequestHandler for AcmeChallengeHandler {
    const PHASE: ngx::http::HttpPhase = ngx::http::HttpPhase::Content;
    type Output = Status;

    fn handler(request: &mut http::Request) -> Self::Output {
        let uri = request.path().to_str().unwrap_or("");
        let token = match uri.rsplit_once('/') {
            Some((_, t)) => t,
            None => return Status::NGX_DECLINED,
        };

        let auth = match ACME_CACHE.get(token) {
            Some(a) => a,
            None => return HTTPStatus::NOT_FOUND.into(),
        };

        request.set_status(HTTPStatus::OK);
        request.set_content_length_n(auth.len());
        request.add_header_out("Content-Type", "text/plain");

        let status = request.send_header();
        if status != Status::NGX_OK || request.header_only() {
            return status;
        }

        let mut buffer = match request.pool().create_buffer_from_str(&auth) {
            Some(b) => b,
            None => return HTTPStatus::INTERNAL_SERVER_ERROR.into(),
        };
        buffer.set_last_buf(true);
        buffer.set_last_in_chain(true);

        let mut chain = ngx_chain_t {
            buf: buffer.as_ngx_buf_mut(),
            next: ptr::null_mut(),
        };

        request.output_filter(&mut chain)
    }
}

unsafe extern "C" fn ngx_http_mikrom_router_acme_handler(r: *mut ngx_http_request_t) -> ngx_int_t {
    let mut request = unsafe { http::Request::from_ngx_http_request(r) };
    AcmeChallengeHandler::handler(&mut request).0
}

extern "C" fn ngx_http_mikrom_router_commands_set_enable(
    cf: *mut ngx_conf_t,
    _cmd: *mut ngx_command_t,
    conf: *mut c_void,
) -> *mut c_char {
    unsafe {
        let conf = &mut *(conf as *mut ModuleConfig);
        let args: &[ngx_str_t] = (*(*cf).args).as_slice();

        let val = match args[1].to_str() {
            Ok(s) => s,
            Err(_) => {
                ngx_conf_log_error!(NGX_LOG_EMERG, cf, "`mikrom_router` argument is not utf-8 encoded");
                return ngx::core::NGX_CONF_ERROR;
            }
        };

        if val.eq_ignore_ascii_case("on") {
            conf.enable = true;
        } else {
            conf.enable = false;
        }
    };

    ngx::core::NGX_CONF_OK
}

extern "C" fn ngx_http_mikrom_router_commands_set_acme_challenge(
    cf: *mut ngx_conf_t,
    _cmd: *mut ngx_command_t,
    _conf: *mut c_void,
) -> *mut c_char {
    unsafe {
        let args: &[ngx_str_t] = (*(*cf).args).as_slice();
        let val = args[1].to_str().unwrap_or("off");

        if val.eq_ignore_ascii_case("on") {
            let cf_ref = &*cf;
            if let Some(clcf) = http::NgxHttpCoreModule::location_conf_mut(cf_ref) {
                clcf.handler = Some(ngx_http_mikrom_router_acme_handler);
            }
        }
    }

    ngx::core::NGX_CONF_OK
}

static NGX_HTTP_MIKROM_ROUTER_MODULE_CTX: ngx_http_module_t = ngx_http_module_t {
    preconfiguration: Some(Module::preconfiguration),
    postconfiguration: Some(Module::postconfiguration),
    create_main_conf: None,
    init_main_conf: None,
    create_srv_conf: None,
    merge_srv_conf: None,
    create_loc_conf: Some(Module::create_loc_conf),
    merge_loc_conf: Some(Module::merge_loc_conf),
};

// Export the module table so NGINX can find it in the .so
ngx::ngx_modules!(ngx_http_mikrom_router_module);

#[unsafe(no_mangle)]
pub static mut ngx_http_mikrom_router_module: ngx_module_t = ngx_module_t {
    ctx: &raw const NGX_HTTP_MIKROM_ROUTER_MODULE_CTX as _,
    commands: unsafe { &raw mut NGX_HTTP_MIKROM_ROUTER_COMMANDS[0] },
    type_: NGX_HTTP_MODULE as _,
    init_master: None,
    init_module: None,
    init_process: Some(ngx_http_mikrom_router_init_process),
    init_thread: None,
    exit_thread: None,
    exit_process: None,
    exit_master: None,
    ..ngx_module_t::default()
};

unsafe extern "C" fn ngx_http_mikrom_router_init_process(_cycle: *mut ngx::ffi::ngx_cycle_t) -> ngx_int_t {
    let slot = unsafe { ngx_process_slot };
    
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");

        let handle = rt.handle().clone();
        {
            let mut handle_guard = TOKIO_HANDLE.write();
            *handle_guard = Some(handle);
        }

        rt.block_on(async {
            dotenvy::dotenv().ok();
            let database_url = match std::env::var("DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!("Mikrom NGINX Module: DATABASE_URL not set");
                    return;
                }
            };

            eprintln!("Mikrom NGINX Module: Worker {} connecting to DB...", slot);
            let pool = match sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Mikrom NGINX Module: Failed to connect to database: {}", e);
                        return;
                    }
                };

            {
                let mut db_guard = DB_POOL.write();
                *db_guard = Some(pool.clone());
            }

            // Only slot 0 handles NATS and certificate dumping
            if slot == 0 {
                let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());
                
                eprintln!("Mikrom NGINX Module: Slot 0 performing initial sync...");
                
                // Initial sync from DB to Cache and Disk
                if let Err(e) = sync_all_from_db(&pool).await {
                    eprintln!("Mikrom NGINX Module: Failed to perform initial sync: {}", e);
                }

                // Start NATS listener
                loop {
                    eprintln!("Mikrom NGINX Module: Connecting to NATS: {}...", nats_url);
                    let nats_client = match async_nats::connect(&nats_url).await {
                        Ok(client) => {
                            eprintln!("Mikrom NGINX Module: Connected to NATS");
                            client
                        },
                        Err(e) => {
                            eprintln!("Mikrom NGINX Module: Failed to connect to NATS, retrying in 5s: {}", e);
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        },
                    };

                    eprintln!("Mikrom NGINX Module: Listening for NATS updates...");
                    if let Err(e) = listen_for_updates(nats_client, pool.clone()).await {
                        eprintln!("Mikrom NGINX Module: NATS listener error: {}, reconnecting in 5s...", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            } else {
                // Non-zero slots just sync from DB to their local cache once
                if let Err(e) = sync_cache_only_from_db(&pool).await {
                    eprintln!("Mikrom NGINX Module: Worker {} failed to sync cache: {}", slot, e);
                }
                
                // Keep the runtime alive
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                }
            }
        });
    });
    Status::NGX_OK.into()
}

async fn sync_cache_only_from_db(db: &PgPool) -> anyhow::Result<()> {
    // Sync Routes
    let routes = sqlx::query("SELECT hostname, target_url FROM routes").fetch_all(db).await?;
    for route in routes {
        use sqlx::Row;
        let hostname: String = route.get("hostname");
        let target_url: String = route.get("target_url");
        ROUTE_CACHE.insert(hostname, target_url);
    }

    // Sync ACME Challenges
    let challenges = sqlx::query("SELECT token, key_auth FROM acme_challenges").fetch_all(db).await?;
    for challenge in challenges {
        use sqlx::Row;
        let token: String = challenge.get("token");
        let key_auth: String = challenge.get("key_auth");
        ACME_CACHE.insert(token, key_auth);
    }

    Ok(())
}

async fn sync_all_from_db(db: &PgPool) -> anyhow::Result<()> {
    sync_cache_only_from_db(db).await?;
    dump_certificates_to_disk(db).await?;
    Ok(())
}

async fn handle_nats_message(
    subject: &str,
    payload: &[u8],
    db: &PgPool,
) -> anyhow::Result<()> {
    let master_key = std::env::var("MASTER_KEY").unwrap_or_default();
    let tls_dir = std::env::var("TLS_DIR").unwrap_or_else(|_| "/tmp/mikrom/ssl".to_string());

    if subject == mikrom_proto::subjects::ROUTER_CONFIG_UPDATED {
        if let Ok(update) = RouterConfigUpdate::decode(payload) {
            sqlx::query("INSERT INTO routes (hostname, target_url, updated_at) VALUES ($1, $2, TO_TIMESTAMP($3)) ON CONFLICT (hostname) DO UPDATE SET target_url = EXCLUDED.target_url, updated_at = EXCLUDED.updated_at")
                .bind(&update.hostname).bind(&update.target_url).bind(update.timestamp).execute(db).await?;
            
            if let Some(target) = update.target_url {
                ROUTE_CACHE.insert(update.hostname, target);
            } else {
                ROUTE_CACHE.invalidate(&update.hostname);
            }
        }
    } else if subject == mikrom_proto::subjects::ROUTER_TLS_CERT_UPDATED {
        if let Ok(update) = TlsCertificateUpdate::decode(payload) {
            // Save to DB
            sqlx::query("INSERT INTO tls_certificates (hostname, cert_chain, private_key, expires_at) VALUES ($1, $2, $3, TO_TIMESTAMP($4)) ON CONFLICT (hostname) DO UPDATE SET cert_chain = EXCLUDED.cert_chain, private_key = EXCLUDED.private_key, expires_at = EXCLUDED.expires_at")
                .bind(&update.hostname).bind(&update.cert_chain).bind(&update.private_key).bind(update.expires_at).execute(db).await?;

            // Decrypt and write to disk for NGINX
            if !master_key.is_empty() {
                match crypto::decrypt(&update.private_key, &master_key) {
                    Ok(decrypted_key) => {
                        let _ = tokio::fs::create_dir_all(&tls_dir).await;
                        let crt_path = format!("{}/{}.crt", tls_dir, update.hostname);
                        let key_path = format!("{}/{}.key", tls_dir, update.hostname);
                        
                        let _ = tokio::fs::write(&crt_path, &update.cert_chain).await;
                        // Use more restrictive permissions for the private key
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let _ = tokio::fs::write(&key_path, &decrypted_key).await;
                            let _ = tokio::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).await;
                        }
                        #[cfg(not(unix))]
                        let _ = tokio::fs::write(&key_path, &decrypted_key).await;

                        eprintln!("Mikrom NGINX Module: Updated TLS files for {}", update.hostname);
                    },
                    Err(e) => eprintln!("Mikrom NGINX Module: Failed to decrypt private key for {}: {}", update.hostname, e),
                }
            } else {
                eprintln!("Mikrom NGINX Module: MASTER_KEY not set, skipping TLS file generation for {}", update.hostname);
            }
        }
    } else if subject == mikrom_proto::subjects::ROUTER_ACME_CHALLENGE_UPDATED {
        if let Ok(update) = AcmeChallengeUpdate::decode(payload) {
            if update.is_delete {
                sqlx::query("DELETE FROM acme_challenges WHERE token = $1").bind(&update.token).execute(db).await?;
                ACME_CACHE.invalidate(&update.token);
            } else {
                sqlx::query("INSERT INTO acme_challenges (token, key_auth, hostname) VALUES ($1, $2, $3) ON CONFLICT (token) DO UPDATE SET key_auth = EXCLUDED.key_auth, hostname = EXCLUDED.hostname")
                    .bind(&update.token).bind(&update.key_auth).bind(&update.hostname).execute(db).await?;
                ACME_CACHE.insert(update.token, update.key_auth);
            }
        }
    }
    Ok(())
}

async fn dump_certificates_to_disk(db: &PgPool) -> anyhow::Result<()> {
    let master_key = std::env::var("MASTER_KEY").unwrap_or_default();
    let tls_dir = std::env::var("TLS_DIR").unwrap_or_else(|_| "/tmp/mikrom/ssl".to_string());

    if master_key.is_empty() {
        eprintln!("Mikrom NGINX Module: MASTER_KEY not set, skipping certificate dump");
        return Ok(());
    }

    let rows = sqlx::query("SELECT hostname, cert_chain, private_key FROM tls_certificates")
        .fetch_all(db)
        .await?;

    let _ = tokio::fs::create_dir_all(&tls_dir).await;

    for row in &rows {
        use sqlx::Row;
        let hostname: String = row.get("hostname");
        let cert_chain: String = row.get("cert_chain");
        let private_key: String = row.get("private_key");

        match crypto::decrypt(&private_key, &master_key) {
            Ok(decrypted_key) => {
                let crt_path = format!("{}/{}.crt", tls_dir, hostname);
                let key_path = format!("{}/{}.key", tls_dir, hostname);
                
                let _ = tokio::fs::write(&crt_path, &cert_chain).await;
                // Use more restrictive permissions for the private key
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::write(&key_path, &decrypted_key).await;
                    let _ = tokio::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).await;
                }
                #[cfg(not(unix))]
                let _ = tokio::fs::write(&key_path, &decrypted_key).await;
            },
            Err(e) => eprintln!("Mikrom NGINX Module: Failed to decrypt private key for {}: {}", hostname, e),
        }
    }

    eprintln!("Mikrom NGINX Module: Dumped {} certificates to disk", rows.len());
    Ok(())
}


pub async fn listen_for_updates(
    nats_client: async_nats::Client,
    db: PgPool,
) -> anyhow::Result<()> {
    let mut config_sub = nats_client.subscribe(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED).await?;
    let mut tls_sub = nats_client.subscribe(mikrom_proto::subjects::ROUTER_TLS_CERT_UPDATED).await?;
    let mut acme_sub = nats_client.subscribe(mikrom_proto::subjects::ROUTER_ACME_CHALLENGE_UPDATED).await?;

    loop {
        tokio::select! {
            Some(msg) = config_sub.next() => {
                let _ = handle_nats_message(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, &msg.payload, &db).await;
            },
            Some(msg) = tls_sub.next() => {
                let _ = handle_nats_message(mikrom_proto::subjects::ROUTER_TLS_CERT_UPDATED, &msg.payload, &db).await;
            },
            Some(msg) = acme_sub.next() => {
                let _ = handle_nats_message(mikrom_proto::subjects::ROUTER_ACME_CHALLENGE_UPDATED, &msg.payload, &db).await;
            },
            else => break,
        }
    }
    Ok(())
}

#[cfg(test)]
pub async fn resolve_acme_challenge_async(token: &str) -> Option<String> {
    let pool_guard = DB_POOL.read();
    let pool = pool_guard.as_ref()?;
    
    sqlx::query_scalar::<_, String>("SELECT key_auth FROM acme_challenges WHERE token = $1")
        .bind(token)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
pub async fn resolve_target_from_db_async(host: &str) -> Option<String> {
    let pool_guard = DB_POOL.read();
    let pool = pool_guard.as_ref()?;
    
    sqlx::query_scalar::<_, String>("SELECT target_url FROM routes WHERE hostname = $1")
        .bind(host)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}


#[cfg(test)]
pub async fn test_handle_nats_message(subject: &str, payload: &[u8], db: &PgPool) -> anyhow::Result<()> {
    handle_nats_message(subject, payload, db).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Executor;
    use std::env;
    use sqlx::{Connection, PgConnection, postgres::PgPoolOptions};

    pub struct TestDb {
        pool: PgPool,
        db_name: String,
        server_url: String,
    }

    impl TestDb {
        pub async fn new() -> Self {
            dotenvy::dotenv().ok();
            let test_url = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
                "postgres://mikrom:mikrom_password@localhost:5432/mikrom_router_test".to_string()
            });

            let (server_url, base_db_name) = split_url(&test_url);
            let db_name = format!("{}_{}", base_db_name, uuid::Uuid::new_v4().simple());
            let maintenance_url = format!("{}/postgres", server_url);

            let mut conn = PgConnection::connect(&maintenance_url)
                .await
                .expect("Failed to connect to maintenance database");

            conn.execute(format!("CREATE DATABASE {}", db_name).as_str())
                .await
                .expect("Failed to create test database");

            let pool_url = format!("{}/{}", server_url, db_name);
            let pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(&pool_url)
                .await
                .expect("Failed to connect to test db");

            sqlx::migrate!("../mikrom-router/migrations")
                .run(&pool)
                .await
                .expect("Failed to run migrations");

            Self {
                pool,
                db_name,
                server_url,
            }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let server_url = self.server_url.clone();
            let db_name = self.db_name.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                rt.block_on(async {
                    let maintenance_url = format!("{}/postgres", server_url);
                    if let Ok(mut conn) = PgConnection::connect(&maintenance_url).await {
                        let _ = conn.execute(format!("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}' AND pid <> pg_backend_pid()", db_name).as_str()).await;
                        let _ = conn.execute(format!("DROP DATABASE IF EXISTS {}", db_name).as_str()).await;
                    }
                });
            });
        }
    }

    fn split_url(url: &str) -> (String, String) {
        let last_slash = url.rfind('/').expect("Invalid database URL");
        let server_url = &url[..last_slash];
        let db_name = &url[last_slash + 1..];
        (server_url.to_string(), db_name.to_string())
    }

    #[tokio::test]
    async fn test_database_resolution() {
        let db = TestDb::new().await;
        
        {
            let mut guard = DB_POOL.write();
            *guard = Some(db.pool.clone());
        }

        sqlx::query("INSERT INTO routes (hostname, target_url) VALUES ($1, $2)")
            .bind("test.mikrom.local")
            .bind("http://127.0.0.1:8080")
            .execute(&db.pool)
            .await
            .unwrap();

        let target = resolve_target_from_db_async("test.mikrom.local").await;
        assert_eq!(target, Some("http://127.0.0.1:8080".to_string()));
    }

    #[tokio::test]
    async fn test_acme_challenge_resolution() {
        let db = TestDb::new().await;
        
        {
            let mut guard = DB_POOL.write();
            *guard = Some(db.pool.clone());
        }

        sqlx::query("INSERT INTO routes (hostname, target_url) VALUES ($1, $2)")
            .bind("test.mikrom.local")
            .bind("http://127.0.0.1:8080")
            .execute(&db.pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO acme_challenges (token, key_auth, hostname) VALUES ($1, $2, $3)")
            .bind("test-token")
            .bind("test-auth")
            .bind("test.mikrom.local")
            .execute(&db.pool)
            .await
            .unwrap();

        let auth = resolve_acme_challenge_async("test-token").await;
        assert_eq!(auth, Some("test-auth".to_string()));
    }

    #[tokio::test]
    async fn test_nats_integration() {
        use mikrom_proto::router::RouterConfigUpdate;
        let db = TestDb::new().await;
        
        {
            let mut guard = DB_POOL.write();
            *guard = Some(db.pool.clone());
        }

        let nats_url = std::env::var("TEST_NATS_URL").unwrap_or_else(|_| "nats://localhost:4223".into());
        
        // Skip if NATS is not available (though in CI it should be)
        let nats_client = match async_nats::connect(&nats_url).await {
            Ok(client) => client,
            Err(_) => {
                eprintln!("Skipping NATS integration test: NATS not available at {}", nats_url);
                return;
            }
        };

        // Start listener in background
        let pool_clone = db.pool.clone();
        let listener_handle = tokio::spawn(async move {
            let _ = listen_for_updates(nats_client, pool_clone).await;
        });

        // Publish update
        let update = RouterConfigUpdate {
            hostname: "nats-integration.local".to_string(),
            target_url: Some("http://integration-backend".to_string()),
            timestamp: 1625097600,
        };
        let mut payload = Vec::new();
        update.encode(&mut payload).unwrap();

        let nats_publisher = async_nats::connect(&nats_url).await.unwrap();
        nats_publisher.publish(mikrom_proto::subjects::ROUTER_CONFIG_UPDATED, payload.into()).await.unwrap();
        nats_publisher.flush().await.unwrap();

        // Wait for listener to process and update DB
        let mut success = false;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if let Some(target) = resolve_target_from_db_async("nats-integration.local").await {
                if target == "http://integration-backend" {
                    success = true;
                    break;
                }
            }
        }

        listener_handle.abort();
        assert!(success, "Failed to receive and process NATS update within timeout");
    }
}
