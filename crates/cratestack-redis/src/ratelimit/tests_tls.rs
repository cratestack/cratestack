#![cfg(all(test, feature = "tls-rustls"))]

use redis::TlsCertificates;

use super::store::RedisRateLimitStore;

/// `build_with_tls` parses and validates certificates synchronously, with
/// no network I/O, so these run offline like the rest of the unit tests.
fn no_certs() -> TlsCertificates {
    TlsCertificates {
        client_tls: None,
        root_cert: None,
    }
}

#[test]
fn open_with_tls_accepts_rediss_scheme_with_system_trust_store() {
    let store = RedisRateLimitStore::open_with_tls("rediss://127.0.0.1:6380", "bank", no_certs());
    assert!(
        store.is_ok(),
        "rediss:// with no explicit certs should fall back to the system trust store"
    );
}

#[test]
fn open_with_tls_accepts_a_private_ca_bundle() {
    // Not a real CA — just PEM-shaped enough to exercise the parsing path;
    // genuine TLS handshake verification needs a live TLS Redis endpoint
    // and isn't something this offline test can cover.
    let root_cert = include_bytes!("../../tests/fixtures/dummy-ca.pem").to_vec();
    let certs = TlsCertificates {
        client_tls: None,
        root_cert: Some(root_cert),
    };
    let store = RedisRateLimitStore::open_with_tls("rediss://127.0.0.1:6380", "bank", certs);
    assert!(
        store.is_ok(),
        "a well-formed PEM root cert should be accepted"
    );
}

#[test]
fn open_with_tls_rejects_non_tls_scheme() {
    let store = RedisRateLimitStore::open_with_tls("redis://127.0.0.1:6379", "bank", no_certs());
    assert!(
        store.is_err(),
        "open_with_tls must reject `redis://` URLs — TLS requires `rediss://`"
    );
}

#[test]
fn open_with_tls_rejects_malformed_root_cert() {
    // `redis`'s PEM scanner only looks for BEGIN/END markers, so bytes with
    // no markers at all are silently treated as "zero certificates" rather
    // than an error. Use a well-formed PEM envelope around base64 that
    // doesn't decode to a valid DER certificate, which does hit the
    // `RootCertStore::add` failure path.
    let bogus_pem = b"-----BEGIN CERTIFICATE-----\nbm90IGEgdmFsaWQgY2VydGlmaWNhdGU=\n-----END CERTIFICATE-----\n".to_vec();
    let certs = TlsCertificates {
        client_tls: None,
        root_cert: Some(bogus_pem),
    };
    let store = RedisRateLimitStore::open_with_tls("rediss://127.0.0.1:6380", "bank", certs);
    assert!(
        store.is_err(),
        "a root cert whose PEM body isn't a valid DER certificate should fail fast, not be silently ignored"
    );
}
