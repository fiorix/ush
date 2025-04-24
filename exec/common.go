package exec

import (
	"bufio"
	"context"
	"io"

	"ush/strutil"
)

// Read reads one line at a time from r and emits the line as a string
// to the returned channel, unbuffered. The returned channel is closed
// when r returns EOF or ctx is cancelled.
//
// Empty lines and lines starting with '#' are ignored, as well as lines
// that match the exclusion set.
func Read(ctx context.Context, r io.Reader, exclude strutil.StringSet) <-chan string {
	targets := make(chan string)

	go func() {
		defer close(targets)

		scanner := bufio.NewScanner(r)
		for scanner.Scan() {
			t := scanner.Text()
			if t == "" || t[0] == '#' || (exclude != nil && exclude.Contains(t)) {
				continue
			}
			select {
			case targets <- t:
			case <-ctx.Done():
				return
			}
		}
	}()

	return targets
}
