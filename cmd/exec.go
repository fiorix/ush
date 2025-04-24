package cmd

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/spf13/cobra"

	"ush/exec"
	"ush/server"
	"ush/strutil"
)

var execFlags = struct {
	exec.JumpSpec
	ExcludeFile   string
	JumpHostsFile string
}{}

func fatalErr(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

func init() {
	flag := execCmd.Flags()
	spec := &execFlags.JumpSpec

	flag.DurationVarP(&spec.Timeout, "timeout", "t", time.Minute, "timeout of each command execution")
	flag.IntVar(&spec.StderrBytes, "stderr_bytes", 4*1024, "number of bytes to read from command's stderr")
	flag.IntVar(&spec.StdoutBytes, "stdout_bytes", 4*1024, "number of bytes to read from command's stdout")
	flag.IntVarP(&spec.Parallel, "parallel", "p", 1, "number of parallel commands to execute")
	flag.StringVar(&spec.JumpCommand, "jump_cmd", exec.DefaultJumpCommand, "jump command where {.J} is replaced with a jump host")
	flag.StringVarP(&execFlags.ExcludeFile, "exclude", "e", "", "file containing target and jump host exclusion list, one per line")
	flag.StringVarP(&execFlags.JumpHostsFile, "jump_hosts", "j", "", "file containing jump hosts, one per line")
	flag.StringVarP(&spec.JumpHostsKeyFile, "jump_key", "k", "", "file containing ssh key to add to ssh-agent")
	flag.StringVarP(&spec.FileOrigin, "file", "f", "", "path to a local file or a remote address `host:port`. Avoid serving large files as whole content is stored in memory and cached")
	flag.StringVarP(&spec.ServeAddress, "address", "l", "localhost:5050", "local address for serving file")

	rootCmd.AddCommand(execCmd)
}

// execCmd represents the exec subcommand.
var execCmd = &cobra.Command{
	Use:   "exec [flags] <command>",
	Short: "Execute parallel commands from standard input",
	Long: `Use ush to compose and execute commands in parallel.

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
`,
	Run: func(cmd *cobra.Command, args []string) {
		if len(args) == 0 {
			cmd.Help()
			os.Exit(1)
		}

		s := &execFlags.JumpSpec
		s.Command = args[0]
		s.Args = args[1:]

		var err error
		var exclude strutil.StringSet
		if execFlags.ExcludeFile != "" {
			exclude, err = strutil.NewStringSetFromFile(execFlags.ExcludeFile)
			if err != nil {
				fatalErr(err)
			}
		}

		ctx := context.Background()
		ctx, cancel := context.WithCancel(ctx)
		defer cancel()

		if execFlags.FileOrigin != "" {
			s, err := server.NewServer(execFlags.FileOrigin, execFlags.ServeAddress)
			if err != nil {
				fatalErr(err)
			}

			errChan, err := s.Serve(ctx)
			if err != nil {
				fatalErr(err)
			}
			go func() {
				err := <-errChan
				switch err {
				case server.ErrNetConnWrite:
					fmt.Fprintln(os.Stderr, err)
				case server.ErrNetConnAccept:
					fatalErr(err)
				default:
					fmt.Fprintln(os.Stderr, fmt.Errorf("unknown error: %v", err))
				}
			}()
		}

		targets := exec.Read(ctx, os.Stdin, exclude)

		if execFlags.JumpHostsFile == "" {
			err := exec.Exec(ctx, os.Stdout, &s.Spec, targets)
			if err != nil {
				fatalErr(err)
			}
			return
		}

		hosts, err := strutil.NewStringSetFromFile(execFlags.JumpHostsFile)
		if err != nil {
			fatalErr(err)
		}

		hosts.Remove(exclude.SortedStrings()...)

		s.JumpHosts = hosts.SortedStrings()

		err = exec.JumpExec(ctx, os.Stdout, s, targets)
		if err != nil {
			fatalErr(err)
		}
	},
}
