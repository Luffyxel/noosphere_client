use std::{collections::HashMap, time::Duration};

use crate::error::{Error, Result};

const SERVICE: &str = "org.freedesktop.Notifications";
const OBJECT: &str = "/org/freedesktop/Notifications";
const TIMEOUT: Duration = Duration::from_secs(5);

pub async fn show(title: &str, body: &str) -> Result<()> {
    tokio::time::timeout(TIMEOUT, async {
        let connection = zbus::Connection::session()
            .await
            .map_err(|_| Error::Local)?;
        send(&connection, title, body).await
    })
    .await
    .map_err(|_| Error::Local)?
}

async fn send(connection: &zbus::Connection, title: &str, body: &str) -> Result<()> {
    let body = body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let actions: Vec<&str> = Vec::new();
    let hints = HashMap::from([("desktop-entry", zbus::zvariant::Value::from("noosphere"))]);
    let reply = connection
        .call_method(
            Some(SERVICE),
            OBJECT,
            Some(SERVICE),
            "Notify",
            &(
                "Noosphere",
                0_u32,
                "noosphere",
                title,
                body,
                actions,
                hints,
                -1_i32,
            ),
        )
        .await
        .map_err(|_| Error::Local)?;
    reply
        .body()
        .deserialize::<u32>()
        .map_err(|_| Error::Local)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead as _, BufReader},
        process::{Child, Command, Stdio},
        sync::{Arc, Mutex},
    };

    use super::*;

    struct Bus {
        child: Child,
        address: String,
        _directory: tempfile::TempDir,
    }

    impl Bus {
        fn start() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let address = format!("unix:path={}", directory.path().join("bus").display());
            let mut child = Command::new("dbus-daemon")
                .args([
                    "--session",
                    "--nofork",
                    "--nopidfile",
                    "--nosyslog",
                    "--print-address=1",
                ])
                .arg(format!("--address={address}"))
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert!(line.starts_with("unix:"));
            Self {
                child,
                address,
                _directory: directory,
            }
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[derive(Default, Clone)]
    struct Notifications(Arc<Mutex<Vec<(String, String)>>>);

    #[allow(clippy::too_many_arguments)]
    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Notifications {
        fn notify(
            &self,
            app_name: &str,
            replaces_id: u32,
            app_icon: &str,
            summary: &str,
            body: &str,
            actions: Vec<String>,
            hints: HashMap<String, zbus::zvariant::OwnedValue>,
            expire_timeout: i32,
        ) -> u32 {
            assert_eq!(app_name, "Noosphere");
            assert_eq!(replaces_id, 0);
            assert_eq!(app_icon, "noosphere");
            assert!(actions.is_empty());
            assert_eq!(
                <&str>::try_from(hints.get("desktop-entry").unwrap()).unwrap(),
                "noosphere"
            );
            assert_eq!(expire_timeout, -1);
            self.0
                .lock()
                .unwrap()
                .push((summary.to_owned(), body.to_owned()));
            42
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn notification_is_delivered_from_a_runtime_worker() {
        let bus = Bus::start();
        let notifications = Notifications::default();
        let received = notifications.0.clone();
        let _server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(OBJECT, notifications)
            .unwrap()
            .build()
            .await
            .unwrap();
        let connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        tokio::spawn(async move {
            tokio::time::timeout(
                TIMEOUT,
                send(&connection, "Nouveau message", "Salut <ami> & café"),
            )
            .await
            .unwrap()
            .unwrap();
        })
        .await
        .unwrap();
        assert_eq!(
            *received.lock().unwrap(),
            vec![(
                "Nouveau message".to_owned(),
                "Salut &lt;ami&gt; &amp; café".to_owned()
            )]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_notification_service_returns_an_error() {
        let bus = Bus::start();
        let connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        assert!(
            tokio::spawn(async move {
                tokio::time::timeout(TIMEOUT, send(&connection, "Noosphere", "Message"))
                    .await
                    .unwrap()
            })
            .await
            .unwrap()
            .is_err()
        );
    }
}
