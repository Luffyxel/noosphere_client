use noosphere_remote::{
    daemon::{self, Bootstrap, Settings, Status},
    signaling::random_id,
};
use std::{
    io::Write as _,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn separate_host_survives_bootstrap_pipe_close_and_rejects_another_client_key() {
    let id: String = random_id().unwrap()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    #[cfg(windows)]
    let endpoint = format!(r"\\.\pipe\noosphere-remote-{id}");
    #[cfg(unix)]
    let endpoint = std::env::temp_dir()
        .join(format!("remote-{id}.sock"))
        .to_string_lossy()
        .into_owned();
    let key = random_id().unwrap();
    let bytes = serde_json::to_vec(&Bootstrap {
        endpoint: endpoint.clone(),
        key,
        settings: Settings::default(),
    })
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_noosphere-remote-host"));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000);
    }
    let mut host = Host(command.spawn().unwrap());
    {
        let mut input = host.0.stdin.take().unwrap();
        input
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        input.write_all(&bytes).unwrap();
    }
    let status = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(status) =
                daemon::ipc::request::<Status>(&endpoint, &key, daemon::Command::Status).await
            {
                break status;
            }
            assert!(host.0.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.pid, host.0.id());
    assert_ne!(status.pid, std::process::id());
    assert!(!status.host_enabled);
    assert!(
        daemon::ipc::request::<Status>(&endpoint, &random_id().unwrap(), daemon::Command::Status)
            .await
            .is_err()
    );
    assert!(
        daemon::ipc::request::<bool>(&endpoint, &key, daemon::Command::Stop)
            .await
            .unwrap()
    );
}
