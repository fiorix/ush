package exec

import (
	"bytes"
	"context"
	"testing"
	"time"
)

func TestJumpExec(t *testing.T) {
	ctx := context.Background()
	s := &JumpSpec{
		Spec: Spec{
			Command:     "echo",
			Args:        []string{"{.T}"},
			Timeout:     1 * time.Second,
			Parallel:    1,
			StdoutBytes: 1024,
			StderrBytes: 1024,
		},
		JumpHostsKeyFile: "",
		JumpCommand:      "echo {.J}",
		JumpHosts:        []string{"localhost"},
	}

	targets := make(chan string, 1)
	targets <- "hello world"
	close(targets)

	var b bytes.Buffer

	err := JumpExec(ctx, &b, s, targets)
	if err != nil {
		t.Fatal(err)
	}

	want := "localhost -- ush exec --timeout=1s --parallel=1 --stdout_bytes=1024 --stderr_bytes=1024 -- echo {.T}\n"
	have := b.String()

	if have != want {
		t.Fatalf("unexpected response:\nwant: %s\nhave: %s\n", want, have)
	}
}
