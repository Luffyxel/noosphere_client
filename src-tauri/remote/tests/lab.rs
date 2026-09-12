use std::{path::PathBuf, process::Command};

#[test]
fn two_processes_complete_an_authenticated_same_account_session() {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("lab-test-{}", std::process::id()));
    let status = Command::new(env!("CARGO_BIN_EXE_noosphere-remote-lab"))
        .args(["--output"])
        .arg(&output)
        .status()
        .expect("start two-node lab");
    assert!(status.success());
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(output.join("report.json")).expect("read lab report"),
    )
    .expect("parse lab report");
    assert_eq!(report["passed"], true);
    assert_eq!(report["distinctMachines"], true);
    assert_eq!(report["guest"]["framesReceived"], 11);
    assert_eq!(report["guest"]["framesDropped"], 1);
    assert_eq!(report["host"]["revokedInputRejected"], true);
}
