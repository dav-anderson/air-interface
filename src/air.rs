//! What both operators know about the AIR server (air_server) itself: the names
//! of the variables it is started with, the nginx server block that puts TLS in
//! front of its WebSocket, and what its health listener answers.
//!
//! The operator's own parts stay with it: where the values come from, the
//! comment block at the top of its nginx file (what to do with the file differs)
//! and the file the environment is written to.

/// The nginx server block: the part both operators write, from `server {` on.
/// It has `{{public_name}}`, `{{public_cert}}`, `{{public_key}}` and
/// `{{ws_addr}}` placeholders, for [`crate::template::expand`].
pub const NGINX_SERVER: &str = include_str!("../templates/nginx-air.conf");

/// The operator's nginx template: its `header` (the comment block that opens the
/// file) and then the server block.
pub fn nginx_template(header: &str) -> String {
    format!("{header}{NGINX_SERVER}")
}

/// The TLS settings, which go together: the address, the certificate and the key.
#[derive(Clone, Copy, Debug)]
pub struct Tls<'a> {
    pub addr: &'a str,
    pub cert: &'a str,
    pub key: &'a str,
}

/// The values air_server is started with.
#[derive(Clone, Copy, Debug)]
pub struct Env<'a> {
    /// `AIR_SECRET_FILE`: the identity that seeds a new store. `None`: air_server
    /// makes one.
    pub identity: Option<&'a str>,
    pub storage: &'a str,
    pub ws_addr: &'a str,
    pub health_addr: &'a str,
    /// Without TLS settings the server gets none: it would listen on :443 as
    /// soon as it had a certificate and a key.
    pub tls: Option<Tls<'a>>,
}

/// Where `AIR_SECRET_FILE` goes among the others. The file each operator writes
/// has always had it in one place, and the file's text is not changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityAt {
    /// Before `AIR_STORAGE`.
    First,
    /// After `AIR_HEALTH_ADDR`, before the TLS settings.
    AfterHealth,
}

impl Env<'_> {
    /// The `AIR_*` variables, in order.
    pub fn pairs(&self, identity_at: IdentityAt) -> Vec<(String, String)> {
        let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
        let identity = self.identity.map(|i| pair("AIR_SECRET_FILE", i));
        let mut out = Vec::new();
        if identity_at == IdentityAt::First {
            out.extend(identity.clone());
        }
        out.push(pair("AIR_STORAGE", self.storage));
        out.push(pair("AIR_WS_ADDR", self.ws_addr));
        out.push(pair("AIR_HEALTH_ADDR", self.health_addr));
        if identity_at == IdentityAt::AfterHealth {
            out.extend(identity);
        }
        if let Some(tls) = &self.tls {
            out.push(pair("AIR_TLS_ADDR", tls.addr));
            out.push(pair("AIR_TLS_CERT", tls.cert));
            out.push(pair("AIR_TLS_KEY", tls.key));
        }
        out
    }
}

/// Whether an air_server health answer is the bare `ok` it sends: status 200 and
/// the body `ok`. (Any other 200, such as a proxy's page, is not air_server.)
pub fn healthy(status: u16, body: &str) -> bool {
    status == 200 && body.trim() == "ok"
}
