use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::{is_shutdown, Spec};

pub const DEFAULT_JUMP_COMMAND: &str = "ssh -A -oBatchMode=yes -oConnectTimeout=10 -- {jump}";

pub struct JumpSpec {
    pub spec: Spec,
    pub jump_hosts_key_file: String,
    pub jump_command: String,
    pub jump_hosts: Vec<String>,
}

impl JumpSpec {
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.spec.validate()?;
        if self.jump_command.is_empty() {
            return Err("jump command not set".into());
        }
        if self.jump_hosts.is_empty() {
            return Err("no jump hosts available".into());
        }
        // Validate jump host names: reject whitespace and option-like strings
        for host in &self.jump_hosts {
            if host.contains(char::is_whitespace) {
                return Err(format!("jump host name contains whitespace: {:?}", host).into());
            }
            if host.starts_with('-') {
                return Err(format!("jump host name starts with dash: {:?}", host).into());
            }
        }
        Ok(())
    }
}

/// Executes commands via jump hosts with ssh-agent per host.
pub fn jump_exec(
    w: Arc<Mutex<Box<dyn Write + Send>>>,
    spec: &JumpSpec,
    input: mpsc::Receiver<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    spec.validate()?;

    let parallel = std::cmp::max(1, spec.spec.parallel / spec.jump_hosts.len());
    let input = Arc::new(Mutex::new(input));
    let mut handles = Vec::new();

    vlog!(
        "[verbose] jump_exec: {} host(s), {} parallel per host",
        spec.jump_hosts.len(),
        parallel
    );

    for host in &spec.jump_hosts {
        let (mut agent_cmd, authsock) = ssh_agent(host)?;
        vlog!(
            "[verbose] jump_exec: ssh-agent started for {}, sock={}",
            host,
            authsock
        );

        if !spec.jump_hosts_key_file.is_empty() {
            ssh_add_key(host, &authsock, &spec.jump_hosts_key_file)?;
            vlog!("[verbose] jump_exec: key loaded for {}", host);
        }

        // Build SSH command: split the template, substitute {jump} into individual args
        // This prevents argument injection via host names containing spaces or -o flags
        let template_parts: Vec<&str> = spec.jump_command.split_whitespace().collect();
        if template_parts.is_empty() {
            return Err("empty jump command".into());
        }

        let ssh_cmd = template_parts[0].to_string();
        let mut args: Vec<String> = template_parts[1..]
            .iter()
            .map(|s| s.replace("{jump}", host))
            .collect();

        // Append the remote ush command after the SSH args
        args.push("ush".to_string());
        args.push("exec".to_string());
        args.push(format!(
            "--timeout={}",
            crate::time::format_go_duration(spec.spec.timeout)
        ));
        args.push(format!("--parallel={}", parallel));
        args.push(format!("--stdout_bytes={}", spec.spec.stdout_bytes));
        args.push(format!("--stderr_bytes={}", spec.spec.stderr_bytes));
        if spec.spec.head {
            args.push("--head".to_string());
        }
        args.push("--".to_string());
        args.push(spec.spec.command.clone());
        args.extend(spec.spec.args.clone());

        vlog!(
            "[verbose] jump_exec: spawning {} {:?} for host {}",
            ssh_cmd,
            args,
            host
        );

        let mut child = Command::new(&ssh_cmd)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .env("SSH_AUTH_SOCK", &authsock)
            .spawn()?;

        let child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();
        let child_stderr = child.stderr.take().unwrap();

        let input = input.clone();
        let w = w.clone();
        let host_name = host.clone();

        // Feed targets to stdin
        let feed_handle = std::thread::spawn(move || {
            let mut stdin = child_stdin;
            loop {
                if is_shutdown() {
                    break;
                }
                let target = {
                    let rx = input.lock().unwrap();
                    rx.recv().ok()
                };
                match target {
                    Some(t) => {
                        if writeln!(stdin, "{}", t).is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            drop(stdin);
        });

        // Read stdout (JSON lines) and write synchronized
        let w2 = w.clone();
        let stdout_handle = std::thread::spawn(move || {
            let reader = BufReader::new(child_stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        if line.is_empty() {
                            continue;
                        }
                        let mut w = w2.lock().unwrap();
                        let _ = writeln!(&mut *w, "{}", line);
                    }
                    Err(_) => break,
                }
            }
        });

        // Read stderr and log
        let host_name2 = host_name.clone();
        let stderr_handle = std::thread::spawn(move || {
            let reader = BufReader::new(child_stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        eprintln!("{}: {}", host_name2, line);
                    }
                    Err(_) => break,
                }
            }
        });

        handles.push(std::thread::spawn(move || {
            let _ = feed_handle.join();
            let _ = stdout_handle.join();
            let _ = stderr_handle.join();
            let _ = child.wait();
            vlog!(
                "[verbose] jump_exec: SSH to {} finished, killing ssh-agent",
                host_name
            );
            let _ = agent_cmd.kill();
            let _ = agent_cmd.wait();
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    Ok(())
}

fn ssh_agent(jumphost: &str) -> Result<(Child, String), Box<dyn std::error::Error>> {
    let mut cmd = Command::new("ssh-agent")
        .args(["-D", "-s"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = cmd.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut first_line = String::new();
    reader.read_line(&mut first_line)?;

    // Parse SSH_AUTH_SOCK=value; export SSH_AUTH_SOCK;
    let parts: Vec<&str> = first_line.splitn(2, '=').collect();
    if parts.len() != 2 {
        let _ = cmd.kill();
        return Err(format!(
            "{}: unexpected output from ssh-agent: {:?}",
            jumphost, first_line
        )
        .into());
    }

    let sock = parts[1].split(';').next().unwrap_or("").to_string();
    if sock.is_empty() {
        let _ = cmd.kill();
        return Err(format!("{}: empty SSH_AUTH_SOCK from ssh-agent", jumphost).into());
    }

    // Retry socket existence check with short timeout
    let mut found = false;
    for attempt in 0..20 {
        if std::path::Path::new(&sock).exists() {
            found = true;
            break;
        }
        vlog!(
            "[verbose] ssh_agent: waiting for socket {} (attempt {})",
            sock,
            attempt + 1
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    if !found {
        let _ = cmd.kill();
        return Err(format!(
            "{}: ssh-agent socket not found after 2s: {}",
            jumphost, sock
        )
        .into());
    }

    Ok((cmd, sock))
}

fn ssh_add_key(
    jumphost: &str,
    authsock: &str,
    keyfile: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("ssh-add")
        .arg(keyfile)
        .env_clear()
        .env("SSH_AUTH_SOCK", authsock)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{}: ssh-add failed: {}", jumphost, stderr.trim()).into());
    }

    Ok(())
}
