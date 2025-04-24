# ush

ush is a standalone program to serialise and parallelise the execution of other programs, similar to GNU parallels.

This tool was created circa 2017 at Facebook to deal with odd incidents in the infrastructure by allowing us to execute parallel ssh onto millions of hosts, directly or through jump hosts. ush stands for ultrashell, and comes from the team maintaining hypershell at the time - the actual service to manage large scale ssh. The name comes from team humour about how we needed a less intense version of hypershell, with no dependencies. Thus the static, single binary, ultrashell.

The freq command was implemented for feature parity with hypershell. The output of the exec command can be stored in a file and processed by the freq command later. The functionality of serving files with exec -f, and the server code, was never really used.

We used ush for a short period of time before most of the issues of the real hypershell were solved in production.

This repo contains a copy of the code as-is, for archival purposes, with no plans to make any changes to it.

## Usage

The command line tool:

```
$ ush
ush v1.1

Usage:
  ush [command]

Available Commands:
  completion  Generate the autocompletion script for the specified shell
  exec        Execute parallel commands from standard input
  freq        Print frequency of events from ush exec JSON output
  help        Help about any command

Flags:
  -h, --help   help for ush

Use "ush [command] --help" for more information about a command.
```

The exec command:

```
$ ush exec
Use ush to compose and execute commands in parallel.

ush works by consuming line-oriented data from stdin, and uses that information
to compose and execute commands from a templated command line.

Each line written to standard input is a target to ush. When you compose your
command, you refer to the target by using the {.T} template markup.

Example running 2 echo commands:

	echo -ne 'hello\nworld\n' | ush exec -- echo {.T}

Example running hostid (or any other command) via ssh using a list of hosts:

	cat hosts.txt | ush exec -- ssh user@{.T} -- hostid

ush provides an execution mode that makes use of jump hosts to amplify its
capabilities of running parallel ssh. For example, using 100 jump hosts doing
1000 ssh sessions each, one can run 100k parallel ssh sessions at once. Scary
but powerful at the same time.

Example using jump hosts:

	cat hosts.txt | ush exec -j jump_hosts.txt -k jump.key -- ssh user@{.T} -- hostid

If -j is specified, ush opens an ssh session to each jump host and runs ush
there with the same <command>. It then pipes a portion of its own stdin to each
jump host, and consumes their results - the execution log, errors.

When using -j, ush spawns a dedicated ssh-agent for each jump host, and adds the
key specified with -k to each one. We made it like this because in this scenario
the ssh-agent was being a bottleneck for highly parallel jobs, e.g. 100k.

The value of -p is always absolute. If you use jump hosts, the value passed to
ush on the jump hosts is adjusted to the absolute value divided by the number of
jump hosts. The rationale is that if you run ush exec -p 10 and you have 2 jump
hosts, each jump host would do 5 parallel executions. If you have more jump
hosts than the value of -p, the value -p on the jump hosts is set to 1.

Usage:
  ush exec [flags] <command>

Flags:
  -l, --address string      local address for serving file (default "localhost:5050")
  -e, --exclude string      file containing target and jump host exclusion list, one per line
  -f, --file host:port      path to a local file or a remote address host:port. Avoid serving large files as whole content is stored in memory and cached
  -h, --help                help for exec
      --jump_cmd string     jump command where {.J} is replaced with a jump host (default "ssh -A -oBatchMode=yes -oConnectTimeout=10 {.J}")
  -j, --jump_hosts string   file containing jump hosts, one per line
  -k, --jump_key string     file containing ssh key to add to ssh-agent
  -p, --parallel int        number of parallel commands to execute (default 1)
      --stderr_bytes int    number of bytes to read from command's stderr (default 4096)
      --stdout_bytes int    number of bytes to read from command's stdout (default 4096)
  -t, --timeout duration    timeout of each command execution (default 1m0s)
```

The freq command:

```
$ ush freq
Use the ush freq command to compute results from ush exec.

Examples:

	echo hello world | ush exec -- echo {.T} | ush freq exitstatus

	echo -ne 'foo\nbar\n' | ush exec -- echo {.T} | ush freq stdout

	for x in {1..3}; do echo $x; done | ush exec -p 3 -- sleep {.T} | ush freq duration 1s

Usage:
  ush freq [command]

Available Commands:
  duration    Print execution duration distribution truncated to value, e.g. 5s
  exitstatus  Print frequency of similar exit status from ush exec JSON output
  stderr      Print frequency of similar stderr from ush exec JSON output
  stdout      Print frequency of similar stdout from ush exec JSON output

Flags:
  -h, --help   help for freq

Use "ush freq [command] --help" for more information about a command.
```
