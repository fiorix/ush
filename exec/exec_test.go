package exec

import (
	"bytes"
	"context"
	"encoding/json"
	"sync"
	"testing"
	"time"
)

func TestExec(t *testing.T) {
	ctx := context.Background()
	s := &Spec{
		Command:     "echo",
		Args:        []string{"{.T}"},
		Timeout:     1 * time.Second,
		Parallel:    1,
		StdoutBytes: 5,
		StderrBytes: 1024,
	}

	targets := make(chan string, 1)
	targets <- "hello world"
	close(targets)

	var b bytes.Buffer

	err := Exec(ctx, &b, s, targets)
	if err != nil {
		t.Fatal(err)
	}

	var r Result
	err = json.NewDecoder(&b).Decode(&r)
	if err != nil {
		t.Fatal(err)
	}

	switch {
	case r.Target != "hello world":
		t.Fatalf("unexpected target: %q", r.Target)
	case r.Stdout != "hello[...]":
		t.Fatalf("unexpected stdout: %q", r.Stdout)
	case r.ExitStatus != 0:
		t.Fatalf("unexpected exit status: %d", r.ExitStatus)
	}
}

func TestSpecValidate(t *testing.T) {
	cases := []struct {
		*Spec
		Err error
	}{
		{Spec: &Spec{}, Err: ErrNoCommand},
		{Spec: &Spec{Command: "a"}, Err: ErrNoTimeout},
		{Spec: &Spec{Command: "a", Timeout: 1}, Err: ErrNoParallel},
		{Spec: &Spec{Command: "a", Timeout: 1, Parallel: 1}, Err: ErrNoStdoutBytes},
		{Spec: &Spec{Command: "a", Timeout: 1, Parallel: 1, StdoutBytes: 1}, Err: ErrNoStderrBytes},
		{Spec: &Spec{Command: "a", Timeout: 1, Parallel: 1, StdoutBytes: 1, StderrBytes: 1}, Err: nil},
	}
	for i, tc := range cases {
		if err := tc.Spec.Validate(); err != tc.Err {
			t.Fatalf("failed %d: %v != %v", i, err, tc.Err)
		}
	}
}

func TestAgent(t *testing.T) {
	ctx := context.Background()
	ctx, cancel := context.WithTimeout(ctx, 1*time.Second)
	defer cancel()

	cmd, sock, err := sshAgent(ctx, "localhost")
	if err != nil {
		t.Fatal(err)
	}

	if sock == "" {
		t.Fatal("no socket")
	}

	cmd.Process.Kill()
}

func TestSynchronizedWriter(t *testing.T) {
	var mu sync.Mutex
	var b bytes.Buffer

	sw := &synchronizedWriter{Writer: &b, Mutex: &mu}
	sw.Write([]byte("hello"))
	sw.Write([]byte("world\n"))
	sw.Write([]byte("foobar"))
	sw.Flush()

	if !bytes.Equal(b.Bytes(), []byte("helloworld\nfoobar")) {
		t.Fatalf("unexpected buffer: %q", b.String())
	}
}
