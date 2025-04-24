package exec

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"io/ioutil"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"

	"golang.org/x/sync/errgroup"
)

// JumpSpec errors.
var (
	ErrNoJumpCommand = errors.New("jump command not set")
	ErrNoJumpHosts   = errors.New("no jump hosts available")
)

// DefaultJumpCommand is the command used to access jump hosts.
const DefaultJumpCommand = "ssh -A -oBatchMode=yes -oConnectTimeout=10 {.J}"

// JumpSpec is the specification of a batch execution using jump hosts.
// Connection to jump hosts use ssh in batch mode.
//
// Each connection to a jump hosts spawns an ssh-agent to parallelize
// authentication. The JumpHostsKeyFile is automatically added to the
// ssh-agent via ssh-add if set.
type JumpSpec struct {
	Spec
	JumpHostsKeyFile string
	JumpCommand      string
	JumpHosts        []string
}

// Validate checks the spec. Returns error if required settings are not set.
func (s *JumpSpec) Validate() error {
	ss := &s.Spec
	if err := ss.Validate(); err != nil {
		return err
	}
	switch {
	case s.JumpCommand == "":
		return ErrNoJumpCommand
	case len(s.JumpHosts) == 0:
		return ErrNoJumpHosts
	}
	return nil
}

// JumpExec executes job s via jump hosts.
func JumpExec(ctx context.Context, w io.Writer, s *JumpSpec, targets <-chan string) error {
	if err := s.Validate(); err != nil {
		return err
	}

	parallel := s.Parallel / len(s.JumpHosts)
	if parallel == 0 {
		parallel = 1 // TODO: warn about too many jump hosts? cut some?
	}

	var mu sync.Mutex
	g, ctx := errgroup.WithContext(ctx)

	for _, host := range s.JumpHosts {
		// ssh-agent is a bottleneck; spawn one per jump host
		agentcmd, authsock, err := sshAgent(ctx, host)
		if err != nil {
			return err
		}

		if s.JumpHostsKeyFile != "" {
			err = sshAddKey(ctx, host, authsock, s.JumpHostsKeyFile)
			if err != nil {
				return fmt.Errorf("ssh-add failed: %v", err)
			}
		}

		// start ssh to jump host
		pr, pw := io.Pipe()
		stdout := &synchronizedWriter{Writer: w, Mutex: &mu}
		stderr := newLogWriter(ctx, host, true)

		cmd := strings.Replace(s.JumpCommand, "{.J}", host, -1)
		args := strings.Split(cmd, " ")
		cmd, args = args[0], args[1:]

		ushargs := []string{
			"--",
			"ush",
			"exec",
			"--timeout=" + s.Timeout.String(),
			"--parallel=" + strconv.Itoa(parallel),
			"--stdout_bytes=" + strconv.Itoa(s.StdoutBytes),
			"--stderr_bytes=" + strconv.Itoa(s.StderrBytes),
			"--",
			s.Command,
		}

		args = append(args, ushargs...)
		args = append(args, s.Args...)

		oscmd := exec.CommandContext(ctx, cmd, args...)
		oscmd.Stdin = pr
		oscmd.Stdout = stdout
		oscmd.Stderr = stderr
		oscmd.Env = []string{"SSH_AUTH_SOCK=" + authsock}

		err = oscmd.Start()
		if err != nil {
			return err
		}

		g.Go(func() error {
			defer pw.Close()
			for {
				select {
				case <-ctx.Done():
					return ctx.Err()
				case t, more := <-targets:
					if !more {
						return nil
					}
					_, err := io.WriteString(pw, t+"\n")
					if err != nil {
						return err
					}
				}
			}
		})

		g.Go(func() error {
			defer agentcmd.Process.Kill()
			defer stdout.Flush()
			return oscmd.Wait()
		})
	}

	return g.Wait()
}

type synchronizedWriter struct {
	io.Writer
	*sync.Mutex
	b bytes.Buffer
}

func (s *synchronizedWriter) Write(p []byte) (int, error) {
	idx := bytes.Index(p, []byte("\n"))
	switch idx {
	case -1:
		return s.b.Write(p)
	case 0:
		return len(p), nil
	}

	b := &s.b
	b.Write(p[:idx])

	s.Mutex.Lock()
	_, err := io.Copy(s.Writer, b)
	s.Mutex.Unlock()
	if err != nil {
		return 0, err
	}

	b.Reset()
	_, err = b.Write(p[idx:])

	return len(p), err
}

func (s *synchronizedWriter) Flush() error {
	s.Mutex.Lock()
	_, err := io.Copy(s.Writer, &s.b)
	s.Mutex.Unlock()
	return err
}

var errAgentTerminated = errors.New("ssh-agent terminated")

// starts an ssh agent, reads the first line from stdout and discard the rest.
// returns the agent's SSH_AUTH_SOCK.
func sshAgent(ctx context.Context, jumphost string) (*exec.Cmd, string, error) {
	authsock := make(chan string, 1)

	cmd := exec.CommandContext(ctx, "ssh-agent", "-D", "-s")
	cmd.Stdout = &sshAgentStdout{authsock: authsock}
	cmd.Stderr = newLogWriter(ctx, jumphost, true)
	err := cmd.Start()
	if err != nil {
		return nil, "", err
	}

	procErr := make(chan error, 1)
	go func() {
		procErr <- cmd.Wait()
		close(procErr)
	}()

	select {
	case s := <-authsock:
		return cmd, s, nil
	case <-ctx.Done():
		cmd.Process.Kill()
		return nil, "", errAgentTerminated
	case err := <-procErr:
		if err == nil {
			err = errAgentTerminated
		}
		return nil, "", err
	}
}

type sshAgentStdout struct {
	authsock chan string
	done     bool
	b        bytes.Buffer
}

func (s *sshAgentStdout) Write(p []byte) (int, error) {
	if s.done {
		return len(p), nil
	}

	// agent output: SSH_AUTH_SOCK=value; export SSH_AUTH_SOCKET\n

	idx := bytes.Index(p, []byte("\n"))
	n, err := s.b.Write(p)
	if err != nil || idx == -1 {
		return n, err
	}

	line1 := bytes.SplitN(s.b.Bytes(), []byte("\n"), 2)[0]
	parts := bytes.SplitN(line1, []byte("="), 2)
	if len(parts) != 2 && !bytes.Equal(parts[0], []byte("SSH_AUTH_SOCK")) {
		return 0, fmt.Errorf("unexpected output from ssh-agent: %q", string(line1))
	}

	sock := string(bytes.SplitN(parts[1], []byte(";"), 2)[0])
	if _, err := os.Stat(sock); err != nil {
		return 0, fmt.Errorf("cannot stat SSH_AUTH_SOCK: %v", err)
	}

	s.authsock <- sock
	s.done = true
	return len(p), nil
}

func sshAddKey(ctx context.Context, jumphost, authsock, authkey string) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	cmd := exec.CommandContext(ctx, "ssh-add", authkey)
	cmd.Stdout = ioutil.Discard
	cmd.Stderr = newLogWriter(ctx, jumphost, false)
	cmd.Env = []string{"SSH_AUTH_SOCK=" + authsock}
	return cmd.Run()
}

func newLogWriter(ctx context.Context, jumphost string, isErr bool) io.Writer {
	pr, pw := io.Pipe()
	go func() {
		scanner := bufio.NewScanner(pr)
		for scanner.Scan() {
			fmt.Fprintf(os.Stderr, "%s: %s", jumphost, scanner.Text())
		}
	}()
	go func() {
		<-ctx.Done()
		pr.Close()
	}()
	return pw
}
