//! Install / uninstall the altkey local CA into the OS trust store.
//! The system command differs per OS; we build it as an argv we can unit-test,
//! then run it. Running requires admin/root and is exercised manually.
use anyhow::{anyhow, Result};
use std::path::Path;

/// (program, args) to install `ca_cert` into the OS trust store.
pub fn install_command(ca_cert: &Path) -> (String, Vec<String>) {
    let p = ca_cert.display().to_string();
    #[cfg(windows)]
    {
        ("certutil".into(), vec!["-addstore".into(), "-f".into(), "Root".into(), p])
    }
    #[cfg(target_os = "macos")]
    {
        (
            "security".into(),
            vec![
                "add-trusted-cert".into(),
                "-d".into(),
                "-r".into(),
                "trustRoot".into(),
                "-k".into(),
                "/Library/Keychains/System.keychain".into(),
                p,
            ],
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ("cp".into(), vec![p, "/usr/local/share/ca-certificates/altkey.crt".into()])
    }
}

/// (program, args) to remove the altkey CA from the OS trust store.
pub fn uninstall_command() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        ("certutil".into(), vec!["-delstore".into(), "Root".into(), "altkey local CA".into()])
    }
    #[cfg(target_os = "macos")]
    {
        ("security".into(), vec!["delete-certificate".into(), "-c".into(), "altkey local CA".into()])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ("rm".into(), vec!["-f".into(), "/usr/local/share/ca-certificates/altkey.crt".into()])
    }
}

/// Run the install command. Returns Ok only on a zero exit code.
pub fn install(ca_cert: &Path) -> Result<()> {
    let (prog, args) = install_command(ca_cert);
    run(&prog, &args)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    run("update-ca-certificates", &[])?;
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let (prog, args) = uninstall_command();
    run(&prog, &args).ok(); // best-effort
    #[cfg(all(unix, not(target_os = "macos")))]
    run("update-ca-certificates", &["--fresh".to_string()]).ok();
    Ok(())
}

fn run(prog: &str, args: &[String]) -> Result<()> {
    let status = std::process::Command::new(prog)
        .args(args)
        .status()
        .map_err(|e| anyhow!("spawn {prog}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{prog} exited {:?}", status.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn install_command_references_the_cert_path() {
        let (prog, args) = install_command(&PathBuf::from("/tmp/altkey/ca.crt"));
        assert!(!prog.is_empty());
        assert!(
            args.iter().any(|a| a.contains("ca.crt")),
            "install argv must include the cert path"
        );
    }

    #[test]
    fn uninstall_command_is_nonempty() {
        let (prog, args) = uninstall_command();
        assert!(!prog.is_empty());
        assert!(!args.is_empty());
    }
}
