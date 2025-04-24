package exec

import (
	"bytes"
	"context"
	"testing"

	"ush/strutil"
)

func TestRead(t *testing.T) {
	ctx := context.Background()
	targets := bytes.NewBuffer([]byte("hello\nworld\n"))

	for line := range Read(ctx, targets, strutil.NewStringSet("world")) {
		if line != "hello" {
			t.Fatalf("unexpected target: %q", line)
		}
	}
}
