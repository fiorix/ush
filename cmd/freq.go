package cmd

import (
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/spf13/cobra"

	"ush/freq"
)

var freqFlags = struct {
	EncodeJSON bool
}{}

func init() {
	rootCmd.AddCommand(freqCmd)

	subcmds := []*cobra.Command{
		freqDurationCmd,
		freqExitStatusCmd,
		freqStderrCmd,
		freqStdoutCmd,
	}

	for _, cmd := range subcmds {
		flag := cmd.Flags()
		flag.BoolVar(&freqFlags.EncodeJSON, "json", false, "encode output as json")
		freqCmd.AddCommand(cmd)
	}
}

// freqCmd represents the freq subcommand.
var freqCmd = &cobra.Command{
	Use:   "freq",
	Short: "Print frequency of events from ush exec JSON output",
	Long: `Use the ush freq command to compute results from ush exec.

Examples:

	echo hello world | ush exec -- echo {.T} | ush freq exitstatus

	echo -ne 'foo\nbar\n' | ush exec -- echo {.T} | ush freq stdout

	for x in {1..3}; do echo $x; done | ush exec -p 3 -- sleep {.T} | ush freq duration 1s
`,
}

func encodeItems(items []freq.Item, err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return
	}

	if freqFlags.EncodeJSON {
		err = freq.EncodeJSON(os.Stdout, items)
	} else {
		err = freq.EncodeWide(os.Stdout, items)
	}

	if err != nil {
		fmt.Fprintln(os.Stderr, err)
	}
}

var freqStdoutCmd = &cobra.Command{
	Use:   "stdout [flags] < results.json",
	Short: "Print frequency of similar stdout from ush exec JSON output",
	Run: func(cmd *cobra.Command, args []string) {
		flag.Parse()
		encodeItems(freq.Stdout(os.Stdin))
	},
}

var freqStderrCmd = &cobra.Command{
	Use:   "stderr [flags] < results.json",
	Short: "Print frequency of similar stderr from ush exec JSON output",
	Run: func(cmd *cobra.Command, args []string) {
		flag.Parse()
		encodeItems(freq.Stderr(os.Stdin))
	},
}

var freqExitStatusCmd = &cobra.Command{
	Use:   "exitstatus [flags] < results.json",
	Short: "Print frequency of similar exit status from ush exec JSON output",
	Run: func(cmd *cobra.Command, args []string) {
		flag.Parse()
		encodeItems(freq.ExitStatus(os.Stdin))
	},
}

var freqDurationCmd = &cobra.Command{
	Use:   "duration [flags] value < results.json",
	Short: "Print execution duration distribution truncated to value, e.g. 5s",
	Run: func(cmd *cobra.Command, args []string) {
		flag.Parse()

		if len(args) == 0 {
			cmd.Help()
			os.Exit(1)
		}

		d, err := time.ParseDuration(args[0])
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			return
		}

		encodeItems(freq.Duration(os.Stdin, d))
	},
}
