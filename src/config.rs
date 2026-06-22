use std::num::{NonZeroU16, NonZeroUsize};

use serde::Deserialize;

const DEFAULT_LISTEN_HOST: &str = "0.0.0.0";
const DEFAULT_LISTEN_PORT: NonZeroU16 = NonZeroU16::new(28883).unwrap();
const DEFAULT_WEB_WORKERS: NonZeroUsize = NonZeroUsize::new(4).unwrap();
const DEFAULT_PAYLOAD_DIR: &str = "data/payloads";
const DEFAULT_HTML_TITLE: &str = "Atlas Payload Provider";
const DEFAULT_MAX_PAYLOAD_BYTES: NonZeroUsize = NonZeroUsize::new(1024 * 1024).unwrap();

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    #[serde(default = "default_listen_port")]
    pub listen_port: NonZeroU16,
    #[serde(default = "default_web_workers")]
    pub web_workers: NonZeroUsize,
    #[serde(default = "default_payload_dir")]
    pub payload_dir: String,
    #[serde(default = "default_html_title")]
    pub html_title: String,
    #[serde(default = "default_max_payload_bytes")]
    pub max_payload_bytes: NonZeroUsize,
    #[serde(default)]
    pub ingress_bearer_key: Option<String>,
    #[serde(default)]
    pub signer_private_key: Option<String>,
}

pub fn create_config() -> Config {
    envy::from_env::<Config>().unwrap_or_else(|err| panic!("invalid config: {err}"))
}

fn default_listen_host() -> String {
    DEFAULT_LISTEN_HOST.to_string()
}

fn default_listen_port() -> NonZeroU16 {
    DEFAULT_LISTEN_PORT
}

fn default_web_workers() -> NonZeroUsize {
    DEFAULT_WEB_WORKERS
}

fn default_payload_dir() -> String {
    DEFAULT_PAYLOAD_DIR.to_string()
}

fn default_html_title() -> String {
    DEFAULT_HTML_TITLE.to_string()
}

fn default_max_payload_bytes() -> NonZeroUsize {
    DEFAULT_MAX_PAYLOAD_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Result<Config, envy::Error> {
        envy::from_iter(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        )
    }

    #[test]
    fn defaults_apply_when_env_is_empty() {
        let config = from_pairs([]).unwrap();
        assert_eq!(config.listen_host, DEFAULT_LISTEN_HOST);
        assert_eq!(config.listen_port, DEFAULT_LISTEN_PORT);
        assert_eq!(config.web_workers, DEFAULT_WEB_WORKERS);
        assert_eq!(config.payload_dir, DEFAULT_PAYLOAD_DIR);
        assert_eq!(config.html_title, DEFAULT_HTML_TITLE);
        assert_eq!(config.max_payload_bytes, DEFAULT_MAX_PAYLOAD_BYTES);
        assert_eq!(config.ingress_bearer_key, None);
        assert_eq!(config.signer_private_key, None);
    }

    #[test]
    fn parses_valid_overrides() {
        let config = from_pairs([
            ("PAYLOAD_DIR", "/var/lib/atlas/payloads"),
            ("HTML_TITLE", "Atlas DA"),
            ("MAX_PAYLOAD_BYTES", "2048"),
            ("INGRESS_BEARER_KEY", "s3cret"),
            ("SIGNER_PRIVATE_KEY", "0xabc"),
        ])
        .unwrap();
        assert_eq!(config.payload_dir, "/var/lib/atlas/payloads");
        assert_eq!(config.html_title, "Atlas DA");
        assert_eq!(config.max_payload_bytes.get(), 2048);
        assert_eq!(config.ingress_bearer_key.as_deref(), Some("s3cret"));
        assert_eq!(config.signer_private_key.as_deref(), Some("0xabc"));
    }

    #[test]
    fn rejects_non_integer_max_payload_bytes() {
        assert!(from_pairs([("MAX_PAYLOAD_BYTES", "abc")]).is_err());
    }

    #[test]
    fn rejects_zero_max_payload_bytes() {
        assert!(from_pairs([("MAX_PAYLOAD_BYTES", "0")]).is_err());
    }
}
