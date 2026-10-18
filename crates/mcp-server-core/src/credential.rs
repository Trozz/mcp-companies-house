//! Per-user credentials. Each person signs in with their own secret for the upstream service the
//! server talks to (for example a Companies House API key), so one server can serve many people.
//!
//! The server stores no credential. It is checked once with the upstream service when the person
//! signs in, then sealed (encrypted and authenticated with a key derived from
//! `OAUTH_SIGNING_KEY`) into their OAuth tokens, and opened again on every request. A client that
//! can send a header may instead send the credential itself as its bearer token; that is checked
//! upstream too, and the result is cached briefly under the credential's SHA-256, never the
//! credential.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::hkdf;
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{OnceCell, Semaphore};

/// Longest credential accepted, as a bound on what is sent upstream and sealed.
pub const MAX_CREDENTIAL_LEN: usize = 256;

/// The caller's credential, placed in the request extensions by the bearer guard.
#[derive(Clone)]
pub struct Credential(SecretString);

impl Credential {
    pub fn new(secret: impl Into<String>) -> Self {
        Credential(SecretString::from(secret.into()))
    }

    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// SHA-256 of the credential: safe to keep as a map key or compare, never to log.
    pub fn fingerprint(&self) -> [u8; 32] {
        fingerprint(self.expose())
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credential(..)")
    }
}

pub fn fingerprint(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

/// The upstream service's verdict on a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialCheck {
    Valid,
    /// Rejected, with a reason to show the person when there is a more useful one than "not
    /// accepted".
    Invalid(Option<String>),
    /// Could not be checked now (upstream down, rate limited): say why, and do not count it as a
    /// failed sign-in.
    Unavailable(String),
}

pub type CheckFuture<'a> = Pin<Box<dyn Future<Output = CredentialCheck> + Send + 'a>>;

/// Checks credentials with the upstream service. Provided by a service; the core asks it on
/// sign-in and for bearer credentials.
pub trait CredentialVerifier: Send + Sync + 'static {
    /// The credential's name on the sign-in page, e.g. "Companies House API key".
    fn label(&self) -> &'static str;
    /// One sentence of help under the field, plain text: what it is and where to get one.
    fn help(&self) -> &'static str;
    /// Whether `credential` has the shape of one, checked before anything is sent upstream.
    /// Defaults to [`plausible`]; narrow it when the upstream service's credentials have a known
    /// form, so other tokens are refused locally instead of being forwarded.
    fn plausible(&self, credential: &str) -> bool {
        plausible(credential)
    }
    /// Checks `credential` with the upstream service.
    fn check<'a>(&'a self, credential: &'a str) -> CheckFuture<'a>;
}

/// Whether `s` has the shape of a credential: non-empty, at most [`MAX_CREDENTIAL_LEN`] bytes,
/// visible ASCII without spaces.
pub fn plausible(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_CREDENTIAL_LEN && s.bytes().all(|b| b.is_ascii_graphic())
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SealError {
    #[error("sealing failed")]
    Seal,
    #[error("the sealed credential is invalid")]
    Open,
}

/// Seals credentials into tokens with ChaCha20-Poly1305 under a key derived (HKDF-SHA256) from
/// the OAuth signing key, so rotating `OAUTH_SIGNING_KEY` also invalidates every sealed
/// credential.
pub struct Sealer {
    key: LessSafeKey,
    rng: SystemRandom,
}

const SEAL_SALT: &[u8] = b"mcp-server-core credential sealing";
const SEAL_INFO: &[u8] = b"v1";
const SEAL_AAD: &[u8] = b"mcp-server-core credential v1";

impl Sealer {
    pub fn new(signing_key: &[u8]) -> Self {
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, SEAL_SALT).extract(signing_key);
        let okm = prk
            .expand(&[SEAL_INFO], &CHACHA20_POLY1305)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        Sealer {
            key: LessSafeKey::new(UnboundKey::from(okm)),
            rng: SystemRandom::new(),
        }
    }

    /// `base64url(nonce || ciphertext || tag)` of `credential`, with a random nonce.
    pub fn seal(&self, credential: &str) -> Result<String, SealError> {
        let mut nonce = [0u8; NONCE_LEN];
        self.rng.fill(&mut nonce).map_err(|_| SealError::Seal)?;
        let mut data = credential.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(SEAL_AAD),
                &mut data,
            )
            .map_err(|_| SealError::Seal)?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&data);
        Ok(URL_SAFE_NO_PAD.encode(out))
    }

    pub fn open(&self, sealed: &str) -> Result<Credential, SealError> {
        let raw = URL_SAFE_NO_PAD
            .decode(sealed)
            .map_err(|_| SealError::Open)?;
        if raw.len() < NONCE_LEN {
            return Err(SealError::Open);
        }
        let (nonce, rest) = raw.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| SealError::Open)?;
        let mut data = rest.to_vec();
        let plain = self
            .key
            .open_in_place(nonce, Aad::from(SEAL_AAD), &mut data)
            .map_err(|_| SealError::Open)?;
        let text = std::str::from_utf8(plain).map_err(|_| SealError::Open)?;
        Ok(Credential::new(text))
    }
}

/// Recent verdicts on bearer credentials, keyed by fingerprint. Valid ones are remembered longer
/// than rejected ones, so a fixed key is not rechecked upstream on every request.
pub struct CheckCache {
    entries: Mutex<HashMap<[u8; 32], (Instant, bool)>>,
    valid_for: Duration,
    invalid_for: Duration,
    max_entries: usize,
}

impl CheckCache {
    pub fn new(valid_for: Duration, invalid_for: Duration, max_entries: usize) -> Self {
        CheckCache {
            entries: Mutex::new(HashMap::new()),
            valid_for,
            invalid_for,
            max_entries,
        }
    }

    /// The cached verdict, if one has not expired.
    pub fn get(&self, fingerprint: &[u8; 32]) -> Option<bool> {
        let now = Instant::now();
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries
            .get(fingerprint)
            .filter(|(expires, _)| *expires > now)
            .map(|(_, valid)| *valid)
    }

    pub fn put(&self, fingerprint: [u8; 32], valid: bool) {
        let now = Instant::now();
        let ttl = if valid {
            self.valid_for
        } else {
            self.invalid_for
        };
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() >= self.max_entries && !entries.contains_key(&fingerprint) {
            entries.retain(|_, (expires, _)| *expires > now);
            // Rejected keys go first, so a burst of junk never pushes out keys that work.
            if entries.len() >= self.max_entries {
                entries.retain(|_, (_, valid)| *valid);
            }
            if entries.len() >= self.max_entries {
                if !valid {
                    return;
                }
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, (expires, _))| *expires)
                    .map(|(k, _)| *k)
                {
                    entries.remove(&oldest);
                }
            }
        }
        entries.insert(fingerprint, (now + ttl, valid));
    }
}

/// Runs upstream credential checks: one check at a time per credential (concurrent requests with
/// the same one share its verdict), at most `max_running` at once, each bounded by `timeout`.
pub struct CheckRunner {
    permits: Semaphore,
    timeout: Duration,
    running: Mutex<HashMap<[u8; 32], Arc<OnceCell<CredentialCheck>>>>,
}

impl CheckRunner {
    pub fn new(max_running: usize, timeout: Duration) -> Self {
        CheckRunner {
            permits: Semaphore::new(max_running),
            timeout,
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Whether a check of this credential is already running, so a new request will share it.
    pub fn is_running(&self, fingerprint: &[u8; 32]) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(fingerprint)
    }

    pub async fn check(
        &self,
        verifier: &dyn CredentialVerifier,
        credential: &str,
    ) -> CredentialCheck {
        let fp = fingerprint(credential);
        let cell = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(fp)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();
        let verdict = cell
            .get_or_init(|| async {
                let Ok(_permit) = self.permits.try_acquire() else {
                    return CredentialCheck::Unavailable(
                        "the server is checking too many keys at once".into(),
                    );
                };
                match tokio::time::timeout(self.timeout, verifier.check(credential)).await {
                    Ok(verdict) => verdict,
                    Err(_) => CredentialCheck::Unavailable("the check timed out".into()),
                }
            })
            .await
            .clone();
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.get(&fp).is_some_and(|c| Arc::ptr_eq(c, &cell)) {
            running.remove(&fp);
        }
        verdict
    }
}

/// A verifier for tests: accepts the listed credentials, reports `unavailable` as
/// [`CredentialCheck::Unavailable`] and rejects everything else. Counts the checks it made.
#[doc(hidden)]
pub struct TestVerifier {
    pub accept: Vec<String>,
    pub unavailable: Option<String>,
    pub checks: std::sync::atomic::AtomicUsize,
}

#[doc(hidden)]
impl TestVerifier {
    pub fn accepting(accept: &[&str]) -> std::sync::Arc<Self> {
        Self::new(accept, None)
    }

    pub fn new(accept: &[&str], unavailable: Option<&str>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(TestVerifier {
            accept: accept.iter().map(|s| s.to_string()).collect(),
            unavailable: unavailable.map(str::to_string),
            checks: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn checks(&self) -> usize {
        self.checks.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl CredentialVerifier for TestVerifier {
    fn label(&self) -> &'static str {
        "API key"
    }

    fn help(&self) -> &'static str {
        "The key for the test service."
    }

    fn check<'a>(&'a self, credential: &'a str) -> CheckFuture<'a> {
        self.checks
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let verdict = if self.unavailable.as_deref() == Some(credential) {
            CredentialCheck::Unavailable("the test service is down".into())
        } else if self.accept.iter().any(|a| a == credential) {
            CredentialCheck::Valid
        } else {
            CredentialCheck::Invalid(None)
        };
        Box::pin(async move { verdict })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn sealed_credentials_open_with_the_same_signing_key_only() {
        let a = Sealer::new(KEY);
        let sealed = a.seal("my-api-key").unwrap();
        assert!(!sealed.contains("my-api-key"));
        assert_eq!(a.open(&sealed).unwrap().expose(), "my-api-key");
        let b = Sealer::new(b"another signing key of 32 bytes!");
        assert_eq!(b.open(&sealed).unwrap_err(), SealError::Open);
    }

    #[test]
    fn sealing_uses_a_fresh_nonce() {
        let s = Sealer::new(KEY);
        assert_ne!(s.seal("k").unwrap(), s.seal("k").unwrap());
    }

    #[test]
    fn tampered_or_malformed_seals_do_not_open() {
        let s = Sealer::new(KEY);
        let sealed = s.seal("my-api-key").unwrap();
        let mut raw = URL_SAFE_NO_PAD.decode(&sealed).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        assert!(s.open(&URL_SAFE_NO_PAD.encode(raw)).is_err());
        for bad in [
            "",
            "not base64!",
            "AAAA",
            &URL_SAFE_NO_PAD.encode([0u8; 12]),
        ] {
            assert!(s.open(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn credentials_never_print() {
        let c = Credential::new("secret-value");
        assert_eq!(format!("{c:?}"), "Credential(..)");
        assert_eq!(c.fingerprint(), fingerprint("secret-value"));
        assert_ne!(c.fingerprint(), fingerprint("secret-valuf"));
    }

    #[test]
    fn plausible_credentials() {
        assert!(plausible("0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0"));
        for bad in [
            "",
            "has space",
            "tab\tkey",
            "ключ",
            &"x".repeat(MAX_CREDENTIAL_LEN + 1),
        ] {
            assert!(!plausible(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_full_cache_drops_rejected_keys_before_working_ones() {
        let c = CheckCache::new(Duration::from_secs(600), Duration::from_secs(60), 3);
        c.put([1; 32], true);
        c.put([2; 32], false);
        c.put([3; 32], false);
        c.put([4; 32], false);
        assert_eq!(
            c.get(&[1; 32]),
            Some(true),
            "junk never pushes out a working key"
        );
        c.put([5; 32], true);
        c.put([6; 32], true);
        c.put([7; 32], true);
        assert!(c.entries.lock().unwrap().len() <= 3);
        assert_eq!(c.get(&[7; 32]), Some(true));
        c.put([8; 32], false);
        assert_eq!(
            c.get(&[8; 32]),
            None,
            "a full cache of working keys keeps them"
        );
        assert_eq!(c.get(&[7; 32]), Some(true));
    }

    struct Slow {
        checks: std::sync::atomic::AtomicUsize,
        delay: Duration,
    }

    impl CredentialVerifier for Slow {
        fn label(&self) -> &'static str {
            "key"
        }
        fn help(&self) -> &'static str {
            ""
        }
        fn check<'a>(&'a self, credential: &'a str) -> CheckFuture<'a> {
            self.checks
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let ok = credential == "good";
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                if ok {
                    CredentialCheck::Valid
                } else {
                    CredentialCheck::Invalid(None)
                }
            })
        }
    }

    fn slow(ms: u64) -> Slow {
        Slow {
            checks: Default::default(),
            delay: Duration::from_millis(ms),
        }
    }

    #[tokio::test]
    async fn concurrent_checks_of_one_credential_share_one_upstream_call() {
        let runner = CheckRunner::new(4, Duration::from_secs(5));
        let v = slow(50);
        let all = futures_join(&runner, &v, &["good"; 8]).await;
        assert!(all.iter().all(|c| *c == CredentialCheck::Valid));
        assert_eq!(v.checks.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!runner.is_running(&fingerprint("good")));
    }

    #[tokio::test]
    async fn running_checks_are_capped_and_timed_out() {
        let runner = CheckRunner::new(2, Duration::from_millis(100));
        let v = slow(300);
        let all = futures_join(&runner, &v, &["a", "b", "c"]).await;
        assert_eq!(v.checks.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(
            all.iter()
                .all(|c| matches!(c, CredentialCheck::Unavailable(_))),
            "{all:?}"
        );
    }

    async fn futures_join(runner: &CheckRunner, v: &Slow, creds: &[&str]) -> Vec<CredentialCheck> {
        let futs = creds.iter().map(|c| runner.check(v, c));
        let mut out = Vec::new();
        let handles: Vec<_> = futs.collect();
        for verdict in join_all(handles).await {
            out.push(verdict);
        }
        out
    }

    async fn join_all<F: Future>(futs: Vec<F>) -> Vec<F::Output> {
        let mut pinned: Vec<Pin<Box<F>>> = futs.into_iter().map(Box::pin).collect();
        let mut done: Vec<Option<F::Output>> = (0..pinned.len()).map(|_| None).collect();
        std::future::poll_fn(|cx| {
            let mut pending = false;
            for (i, f) in pinned.iter_mut().enumerate() {
                if done[i].is_none() {
                    match f.as_mut().poll(cx) {
                        std::task::Poll::Ready(v) => done[i] = Some(v),
                        std::task::Poll::Pending => pending = true,
                    }
                }
            }
            if pending {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        })
        .await;
        done.into_iter().map(|d| d.unwrap()).collect()
    }

    #[test]
    fn the_cache_expires_and_stays_bounded() {
        let c = CheckCache::new(Duration::from_secs(60), Duration::ZERO, 2);
        c.put([1; 32], true);
        c.put([2; 32], false);
        assert_eq!(c.get(&[1; 32]), Some(true));
        assert_eq!(c.get(&[2; 32]), None, "a zero ttl expires at once");
        c.put([3; 32], true);
        c.put([4; 32], true);
        assert!(c.entries.lock().unwrap().len() <= 2);
        assert_eq!(c.get(&[4; 32]), Some(true));
    }
}
