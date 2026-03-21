use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};

use super::Spec;

pub const DEFAULT_JUMP_COMMAND: &str = "ssh -A -oBatchMode=yes -oConnectTimeout=10 {jump}";

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

    for host in &spec.jump_hosts {
        let (mut agent_cmd, authsock) = ssh_agent(host)?;

        if !spec.jump_hosts_key_file.is_empty() {
            ssh_add_key(host, &authsock, &spec.jump_hosts_key_file)?;
        }

        let cmd_str = spec.jump_command.replace("{jump}", host);
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() {
            return Err("empty jump command".into());
        }

        let mut args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
        args.push("--".to_string());
        args.push("ush".to_string());
        args.push("exec".to_string());
        args.push(format!("--timeout={}", crate::time::format_go_duration(spec.spec.timeout)));
        args.push(format!("--parallel={}", parallel));
        args.push(format!("--stdout_bytes={}", spec.spec.stdout_bytes));
        args.push(format!("--stderr_bytes={}", spec.spec.stderr_bytes));
        args.push("--".to_string());
        args.push(spec.spec.command.clone());
        args.extend(spec.spec.args.clone());

        let mut child = Command::new(parts[0])
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
        let stderr_handle = std::thread::spawn(move || {
            let reader = BufReader::new(child_stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        eprintln!("{}: {}", host_name, line);
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
            let _ = agent_cmd.kill();
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    Ok(())
}

fn ssh_agent(_jumphost: &str) -> Result<(Child, String), Box<dyn std::error::Error>> {
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
        return Err(format!("unexpected output from ssh-agent: {:?}", first_line).into());
    }

    let sock = parts[1].split(';').next().unwrap_or("").to_string();
    if sock.is_empty() {
        let _ = cmd.kill();
        return Err("empty SSH_AUTH_SOCK from ssh-agent".into());
    }

    if !std::path::Path::new(&sock).exists() {
        let _ = cmd.kill();
        return Err(format!("cannot stat SSH_AUTH_SOCK: {}", sock).into());
    }

    Ok((cmd, sock))
}

fn ssh_add_key(_jumphost: &str, authsock: &str, keyfile: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("ssh-add")
        .arg(keyfile)
        .env("SSH_AUTH_SOCK", authsock)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;

    if !status.success() {
        return Err(format!("ssh-add failed with status {}", status).into());
    }

    Ok(())
}
