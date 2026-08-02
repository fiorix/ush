# SDME end-to-end tests

These tests run ush inside an outer SDME container. The outer container runs sdme itself, so the hardened target containers are nested. This exercises the real SSH path without depending on external hosts.

## Direct SSH test

`e2e-sdme.sh` builds an Ubuntu rootfs with openssh-server, starts N hardened nested containers with veth networking, and runs `ush exec` over SSH to collect hostnames.

```mermaid
flowchart LR
    host[host]
    outer[outer SDME container]
    targets[hardened target containers]
    ssh[ssh test@host hostname]
    host --> outer
    outer --> targets
    outer --> ssh
```

## Jump-host test

`e2e-sdme-jump.sh` starts two hardened jump containers and N hardened targets in each of two groups (a/b). The outer container runs `ush exec -j jumps -k key` so each jump host runs `ush exec` and fans out SSH to its share of the targets.

```mermaid
flowchart LR
    host[host]
    outer[outer SDME container]
    jump_a[jump host a]
    jump_b[jump host b]
    targets_a[target group a]
    targets_b[target group b]
    host --> outer
    outer --> jump_a
    outer --> jump_b
    jump_a --> targets_a
    jump_b --> targets_b
```

## Running

Both default to N=10 and clean up on success.

```sh
./test/e2e-sdme.sh [N]
./test/e2e-sdme-jump.sh [N]
```

Set `KEEP=1` to leave containers running for inspection.

```sh
KEEP=1 ./test/e2e-sdme-jump.sh 5
```

## Requirements

- Linux host with sdme, systemd-nspawn, and btrfs storage for nested containers.
- Root privileges (sdme operations are rootful).
- jq, ssh-keygen, cargo on the host.
- Enough inotify instances for the nested containers; increase if boot fails:

```sh
sudo sysctl -w fs.inotify.max_user_instances=1024
```
