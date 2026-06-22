mod config;
mod frontend;
mod model;
mod server;
mod signer;
mod store;
mod validation;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;

use config::create_config;
use signer::EthereumSigner;
use store::PayloadStore;

fn main() {
    install_process_panic_handler();

    match startup_action(std::env::args_os().skip(1)) {
        StartupAction::Run => {}
        StartupAction::PrintVersion => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        StartupAction::Error(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }

    let config = create_config();
    let payload_dir = PathBuf::from(&config.payload_dir);
    let max_payload_bytes = config.max_payload_bytes.get();
    let signer = config
        .signer_private_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(EthereumSigner::from_private_key_hex)
        .transpose()
        .unwrap_or_else(|error| panic!("invalid signer private key: {error}"));
    let signer_address = signer.as_ref().map(|signer| signer.address().to_string());
    let store = Arc::new(
        PayloadStore::load(payload_dir.clone(), max_payload_bytes, signer)
            .unwrap_or_else(|error| panic!("failed to load payload store: {error}")),
    );

    let ingress_key = config
        .ingress_bearer_key
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    {
        let snapshot = store.snapshot(8);
        println!(
            "{}",
            serde_json::json!({
                "message": "loaded payload store",
                "payloadDir": payload_dir.display().to_string(),
                "payloadCount": snapshot.payload_count,
                "totalBytes": snapshot.total_bytes,
                "maxPayloadBytes": snapshot.max_payload_bytes,
                "ingressProtected": ingress_key.is_some(),
                "signingEnabled": signer_address.is_some(),
                "signerAddress": signer_address,
            })
        );
    }

    let worker_threads = config.web_workers.get();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_io()
        .enable_time()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(async move {
        let app_state = server::AppState {
            store,
            payload_dir: Arc::new(config.payload_dir.clone()),
            html_title: Arc::new(config.html_title.clone()),
            ingress_key: ingress_key.map(Arc::new),
        };

        server::run_server(
            app_state,
            config.listen_host.clone(),
            config.listen_port.get(),
        )
        .await;
    });
}

fn install_process_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("uncaught panic: {panic_info}");
    }));
}

#[derive(Debug, PartialEq, Eq)]
enum StartupAction {
    Run,
    PrintVersion,
    Error(String),
}

fn startup_action<I>(args: I) -> StartupAction
where
    I: IntoIterator<Item = OsString>,
{
    let mut saw_version = false;

    for arg in args {
        if arg == OsStr::new("-v") || arg == OsStr::new("--version") {
            saw_version = true;
            continue;
        }

        return StartupAction::Error(format!(
            "unsupported command-line argument: {}. Use environment variables to configure atlas-payload-provider; command-line arguments are not supported.",
            arg.to_string_lossy()
        ));
    }

    if saw_version {
        StartupAction::PrintVersion
    } else {
        StartupAction::Run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_runs_service() {
        assert_eq!(startup_action(args(&[])), StartupAction::Run);
    }

    #[test]
    fn version_arguments_print_version() {
        assert_eq!(startup_action(args(&["-v"])), StartupAction::PrintVersion);
        assert_eq!(
            startup_action(args(&["--version"])),
            StartupAction::PrintVersion
        );
    }

    #[test]
    fn invalid_argument_returns_error() {
        match startup_action(args(&["--payload-dir", "/tmp/payloads"])) {
            StartupAction::Error(message) => {
                assert!(message.contains("--payload-dir"));
                assert!(message.contains("environment variables"));
            }
            action => panic!("expected error action, got {action:?}"),
        }
    }

    #[test]
    fn version_with_invalid_argument_returns_error() {
        assert!(matches!(
            startup_action(args(&["--version", "--payload-dir"])),
            StartupAction::Error(_)
        ));
    }
}
