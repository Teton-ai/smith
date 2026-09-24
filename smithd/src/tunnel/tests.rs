#[tokio::test]
async fn tunnel_retries_until_server_is_ready_and_uses_secret() {
    use crate::magic::MagicHandle;
    use crate::shutdown::ShutdownHandler;
    use bore_cli::server::Server;
    use rand::Rng;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::net::TcpListener;
    use tokio::time::Duration;

    fn generate_random_string(length: usize) -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();

        (0..length)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    let random_secret = generate_random_string(10);

    let mut file = NamedTempFile::new().expect("Failed to create temporary file");
    let mocked_magic = format!(
        r#"
[meta]
magic_version = 2
server 		  = "https://api.smith.teton.ai/smith"

[tunnel]
server = "localhost"
secret = "{random_secret}"
"#
    );
    file.write_all(mocked_magic.as_bytes())
        .expect("Failed to write to temporary file");

    let path = file
        .path()
        .to_str()
        .expect("Failed to convert path to string")
        .into();

    let server = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        Server::new(1024..=65535, Some(&random_secret))
            .listen()
            .await
    });

    let listener = TcpListener::bind("localhost:0")
        .await
        .expect("Couldn't bind");
    let local_port = listener
        .local_addr()
        .expect("Couldn't get local address")
        .port();

    let shutdown = ShutdownHandler::new();
    let configuration = MagicHandle::new(shutdown.signals());
    configuration.load(Some(path)).await;
    let tunnel = super::TunnelHandle::new(shutdown.signals(), configuration);

    let resp = tunnel.start_tunnel(Some(local_port), None, None).await;

    assert_ne!(resp, 0);
    server.abort();
}
