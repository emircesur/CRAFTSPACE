//! The shared HTTP client.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig};
use ureq::Agent;

pub const USER_AGENT: &str =
    concat!("CraftSpace/", env!("CARGO_PKG_VERSION"), " (+https://github.com/emircesur/craftspace)");

/// An HTTP agent that uses the operating system's certificate store (so corporate TLS proxies
/// work) and honours `HTTPS_PROXY` / `NO_PROXY`. Responses are returned whatever their status;
/// callers check it.
pub fn agent() -> Agent {
    Agent::config_builder()
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .max_redirects(10)
        .build()
        .into()
}

/// Turn a non-2xx status into an error that names the URL.
pub fn check_status(url: &str, status: u16) -> anyhow::Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        anyhow::bail!("{url} returned HTTP {status}")
    }
}
