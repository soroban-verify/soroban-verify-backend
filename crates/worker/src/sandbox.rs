//! Sandboxed reproducible builds.
//!
//! The build runs untrusted code by design — this is the core security
//! surface of the whole service. The sandbox is two docker invocations:
//!
//!   1. `cargo fetch` — network allowed (dependency download only).
//!      TODO(M4): route through an egress-filtering proxy that only permits
//!      crates.io / the repo's registry.
//!   2. `stellar contract build` — fully offline (`--network=none`,
//!      `CARGO_NET_OFFLINE=true`), CPU/memory/pid capped, caps dropped.
//!
//! Hardening TODOs are tracked in SECURITY.md (rootless runtime or gVisor,
//! per-build user namespaces, named containers with guaranteed teardown on
//! timeout, seccomp profile).

use std::path::{Path, PathBuf};
use std::process::Stdio;

use soroban_verify_common::models::BuildConfig;
use soroban_verify_common::{Error, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::logger::BuildLog;

const RESOURCE_CAPS: &[&str] = &[
    "--cpus=2",
    "--memory=4g",
    "--pids-limit=512",
    "--security-opt=no-new-privileges",
    "--cap-drop=ALL",
    "--tmpfs",
    "/tmp",
];

/// Rebuilds the contract from `src` inside `image` and returns the Wasm bytes.
pub async fn build(
    src: &Path,
    image: &str,
    cfg: &BuildConfig,
    log: &mut BuildLog,
) -> Result<Vec<u8>> {
    let mount = format!("{}:/workspace", src.display());

    log.line("fetching dependencies (network-enabled phase)")
        .await;
    run_docker(
        &docker_args(&mount, image, &["cargo", "fetch", "--locked"], false),
        log,
    )
    .await?;

    let mut build_cmd: Vec<String> = vec!["stellar".into(), "contract".into(), "build".into()];
    if let Some(pkg) = &cfg.package {
        build_cmd.push("--package".into());
        build_cmd.push(pkg.clone());
    }
    if !cfg.features.is_empty() {
        build_cmd.push("--features".into());
        build_cmd.push(cfg.features.join(","));
    }

    log.line("building contract (offline, sandboxed phase)")
        .await;
    let build_cmd_refs: Vec<&str> = build_cmd.iter().map(String::as_str).collect();
    run_docker(&docker_args(&mount, image, &build_cmd_refs, true), log).await?;

    let wasm_path = locate_wasm(src, cfg.package.as_deref())?;
    log.line(format!("built artifact: {}", wasm_path.display()))
        .await;
    Ok(std::fs::read(wasm_path)?)
}

fn docker_args(mount: &str, image: &str, cmd: &[&str], offline: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["run".into(), "--rm".into()];
    args.extend(RESOURCE_CAPS.iter().map(|s| s.to_string()));
    if offline {
        args.push("--network=none".into());
        args.push("-e".into());
        args.push("CARGO_NET_OFFLINE=true".into());
    }
    args.extend(["-v".into(), mount.into(), "-w".into(), "/workspace".into()]);
    args.push(image.into());
    args.extend(cmd.iter().map(|s| s.to_string()));
    args
}

/// Runs a docker command, streaming stdout/stderr line-by-line into the
/// persistent build log.
async fn run_docker(args: &[String], log: &mut BuildLog) -> Result<()> {
    let mut child = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // If the surrounding pipeline times out and drops this future, take
        // the docker client down with it. TODO(M2): also `docker kill` the
        // container itself via a --name handle.
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::Build(format!("failed to spawn docker: {e}")))?;

    let (tx, mut rx) = mpsc::channel::<String>(256);

    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).await.is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    while let Some(line) = rx.recv().await {
        log.line(line).await;
    }

    let status = child.wait().await?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Build(format!("build step failed with {status}")))
    }
}

/// Finds the built Wasm under the standard soroban target dirs.
fn locate_wasm(src: &Path, package: Option<&str>) -> Result<PathBuf> {
    let target_dirs = [
        src.join("target/wasm32-unknown-unknown/release"),
        src.join("target/wasm32v1-none/release"),
    ];

    let mut candidates: Vec<PathBuf> = Vec::new();
    for dir in &target_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wasm") {
                candidates.push(path);
            }
        }
    }

    if let Some(pkg) = package {
        let wanted = format!("{}.wasm", pkg.replace('-', "_"));
        return candidates
            .into_iter()
            .find(|p| p.file_name().is_some_and(|f| f == wanted.as_str()))
            .ok_or_else(|| Error::Build(format!("built wasm {wanted} not found in target dir")));
    }

    match candidates.len() {
        0 => Err(Error::Build(
            "no .wasm artifact produced by the build".into(),
        )),
        1 => Ok(candidates.remove(0)),
        n => Err(Error::Build(format!(
            "{n} wasm artifacts produced — specify build_config.package to disambiguate"
        ))),
    }
}
