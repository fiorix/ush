package exec

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os/exec"
	"strings"
	"sync/atomic"
	"syscall"
	"time"

	"golang.org/x/sync/errgroup"
)

// Spec errors.
var (
	ErrNoCommand     = errors.New("command not set")
	ErrNoTimeout     = errors.New("timeout must be greater than zero")
	ErrNoParallel    = errors.New("parallel must be greater than zero")
	ErrNoStdoutBytes = errors.New("stdout_bytes must be greater than zero")
	ErrNoStderrBytes = errors.New("stderr_bytes must be greater than zero")
)

// Spec is the specification for a batch execution.
type Spec struct {
	Command      string
	Args         []string
	Timeout      time.Duration
	Parallel     int
	StdoutBytes  int
	StderrBytes  int
	FileOrigin   string
	ServeAddress string
}

// Validate checks the spec. Returns error if required settings are not set.
func (s *Spec) Validate() error {
	switch {
	case s.Command == "":
		return ErrNoCommand
	case s.Timeout <= 0:
		return ErrNoTimeout
	case s.Parallel <= 0:
		return ErrNoParallel
	case s.StdoutBytes <= 0:
		return ErrNoStdoutBytes
	case s.StderrBytes <= 0:
		return ErrNoStderrBytes
	default:
		return nil
	}
}

// Result represents the result of a command that was executed.
type Result struct {
	Target     string    `json:"target"`
	Duration   string    `json:"duration"`
	StartTime  time.Time `json:"start_time"`
	EndTime    time.Time `json:"end_time"`
	ExitStatus int       `json:"exit_status"`
	Stdout     string    `json:"stdout"`
	Stderr     string    `json:"stderr"`
	Err        string    `json:"error"`
}

// Exec executes job s on targets until the targets channel is closed or
// ctx is cancelled. Markup {.T} in s.Command is replaced with each input.
func Exec(ctx context.Context, w io.Writer, s *Spec, input <-chan string) error {
	if err := s.Validate(); err != nil {
		return err
	}

	g, ctx := errgroup.WithContext(ctx)
	encoder := json.NewEncoder(w)

	for i := 0; i < s.Parallel; i++ {
		g.Go(func() error {
			for {
				select {
				case <-ctx.Done():
					return ctx.Err()
				case t, more := <-input:
					if !more {
						return nil
					}
					result := runCmd(ctx, s, t)
					encoder.Encode(result)
				}
			}
		})
	}

	return g.Wait()
}

func runCmd(ctx context.Context, s *Spec, target string) Result {
	res := Result{
		Target:    target,
		StartTime: time.Now(),
	}

	var stdout, stderr bytes.Buffer

	cmd := strings.Replace(s.Command, "{.T}", target, -1)
	args := make([]string, 0, len(s.Args))
	for _, arg := range s.Args {
		args = append(args, strings.Replace(arg, "{.T}", target, -1))
	}

	oscmd := exec.Command(cmd, args...)
	oscmd.Stdout = &lossyWriter{Limit: s.StdoutBytes, Buffer: &stdout}
	oscmd.Stderr = &lossyWriter{Limit: s.StderrBytes, Buffer: &stderr}
	oscmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}

	ctx, cancel := context.WithTimeout(ctx, s.Timeout)
	defer cancel()

	shouldKill := int64(1)

	go func() {
		<-ctx.Done()
		if atomic.LoadInt64(&shouldKill) == 1 && oscmd.Process != nil {
			syscall.Kill(-oscmd.Process.Pid, syscall.SIGKILL)
		}
	}()

	if err := oscmd.Run(); err != nil {
		res.Err = err.Error()
		if exitErr, ok := err.(*exec.ExitError); ok {
			if status, ok := exitErr.Sys().(syscall.WaitStatus); ok {
				res.ExitStatus = status.ExitStatus()
			}
		}
	}

	atomic.AddInt64(&shouldKill, -1)

	res.EndTime = time.Now()
	res.Duration = res.EndTime.Sub(res.StartTime).String()
	res.Stdout = stdout.String()
	res.Stderr = stderr.String()
	return res
}

// TODO: make lossyWriter optionally record the last bytes, not the first.
type lossyWriter struct {
	Limit  int
	Buffer *bytes.Buffer
}

// Write writes up to w.Limit bytes to w.Buffer.
// Appends '[...]' to w.Buffer when the limit is reached.
func (w *lossyWriter) Write(p []byte) (int, error) {
	writeSize := len(p)
	bufferSize := w.Buffer.Len()
	if bufferSize >= w.Limit {
		return writeSize, nil
	}
	limit := writeSize
	nextBufferSize := bufferSize + writeSize
	if nextBufferSize > w.Limit {
		limit = w.Limit - bufferSize
	}
	n, err := w.Buffer.Write(p[0:limit])
	if err != nil {
		return n, err
	}
	if w.Buffer.Len() == w.Limit {
		w.Buffer.WriteString("[...]")
	}
	return writeSize, nil
}
