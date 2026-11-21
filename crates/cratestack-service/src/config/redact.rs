//! A `Debug` for [`ServiceConfig`] that never prints a credential.

use std::fmt;

use super::ServiceConfig;

const REDACTED: &str = "<redacted>";

/// A connection URL with its password replaced; a value that does not parse
/// as a URL is hidden entirely, since it may be a DSN holding a password.
fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            if parsed.password().is_some() {
                let _ = parsed.set_password(Some(REDACTED));
            }
            parsed.to_string()
        }
        Err(_) => REDACTED.to_owned(),
    }
}

impl fmt::Debug for ServiceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = f.debug_struct("ServiceConfig");
        out.field("service_name", &self.service_name)
            .field("host", &self.host)
            .field("port", &self.port);
        #[cfg(feature = "postgres")]
        out.field("database_url", &redact_url(&self.database_url));
        out.field("redis_url", &self.redis_url.as_deref().map(redact_url))
            .field("object_storage_endpoint", &self.object_storage_endpoint)
            .field("object_storage_public_url", &self.object_storage_public_url)
            .field("object_storage_bucket", &self.object_storage_bucket)
            .field("object_storage_access_key", &self.object_storage_access_key)
            .field(
                "object_storage_secret_key",
                &self.object_storage_secret_key.as_ref().map(|_| REDACTED),
            )
            .field("public_base_url", &self.public_base_url)
            .field("env_prefix", &self.env_prefix)
            .finish()
    }
}
