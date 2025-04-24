package freq

import (
	"bytes"
	"encoding/json"
	"io"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"ush/exec"
)

func encodeResults(results []exec.Result) *bytes.Buffer {
	var b bytes.Buffer
	enc := json.NewEncoder(&b)
	for _, result := range results {
		enc.Encode(&result)
	}
	return &b
}

func equalSlice(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := 0; i < len(a); i++ {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

type freqFunc func(io.Reader) ([]Item, error)

func TestDuration(t *testing.T) {
	results := encodeResults([]exec.Result{
		{Target: "a", Duration: "8ms"},
		{Target: "b", Duration: "6ms"},
		{Target: "c", Duration: "2ms"},
	})

	items, err := Duration(results, 5*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}

	switch {
	case len(items) != 2:
		t.Fatalf("unexpected number of items: want 2; have %d", len(items))
	case !equalSlice(items[0].Targets, []string{"c"}):
		t.Fatalf("unexpected targets for item: want c; have %+v", items[0].Targets)
	case !equalSlice(items[1].Targets, []string{"a", "b"}):
		t.Fatalf("unexpected targets for item: want a, b; have %+v", items[1].Targets)
	}
}

func TestExitStatus(t *testing.T) {
	results := encodeResults([]exec.Result{
		{Target: "a", ExitStatus: 255},
		{Target: "b", ExitStatus: 255},
		{Target: "c", ExitStatus: 1},
	})

	items, err := ExitStatus(results)
	if err != nil {
		t.Fatal(err)
	}

	switch {
	case len(items) != 2:
		t.Fatalf("unexpected number of items: want 2; have %d", len(items))
	case !equalSlice(items[0].Targets, []string{"c"}):
		t.Fatalf("unexpected targets for item: want c; have %+v", items[0].Targets)
	case !equalSlice(items[1].Targets, []string{"a", "b"}):
		t.Fatalf("unexpected targets for item: want a, b; have %+v", items[1].Targets)
	}
}

func TestStdoutStderr(t *testing.T) {
	results := encodeResults([]exec.Result{
		{Target: "a", Stdout: "hello", Stderr: "foobar"},
		{Target: "b", Stdout: "hello", Stderr: "foobar"},
		{Target: "c", Stdout: "world", Stderr: ""},
	})

	for _, f := range []freqFunc{Stderr, Stdout} {
		n := runtime.FuncForPC(reflect.ValueOf(f).Pointer()).Name()
		n = strings.Split(n, ".")[1]
		t.Run(n, func(t *testing.T) {
			items, err := f(bytes.NewBuffer(results.Bytes()))
			if err != nil {
				t.Fatal(err)
			}

			switch {
			case len(items) != 2:
				t.Fatalf("unexpected number of items: want 2; have %d", len(items))
			case !equalSlice(items[0].Targets, []string{"c"}):
				t.Fatalf("unexpected targets for item: want c; have %+v", items[0].Targets)
			case !equalSlice(items[1].Targets, []string{"a", "b"}):
				t.Fatalf("unexpected targets for item: want a, b; have %+v", items[1].Targets)
			}
		})
	}
}

func TestEncodeJSON(t *testing.T) {
	var b bytes.Buffer
	items := []Item{
		{
			Freq:    100,
			Value:   "hello",
			Targets: []string{"a", "b"},
		},
	}

	if err := EncodeJSON(&b, items); err != nil {
		t.Fatal(err)
	}

	have := b.String()
	want := `{"freq":100,"value":"hello","targets":["a","b"]}` + "\n"
	if have != want {
		t.Fatalf("unexpected buffer:\nwant: %q\nhave: %q\n", want, have)
	}
}

func TestEncodeWide(t *testing.T) {
	var b bytes.Buffer
	items := []Item{
		{
			Freq:    100,
			Value:   "hello",
			Targets: []string{"a", "b"},
		},
	}

	if err := EncodeWide(&b, items); err != nil {
		t.Fatal(err)
	}

	have := b.String()
	want := "count    targets  freq %   value\n1        2        100.00   \"hello\"\n"
	if have != want {
		t.Fatalf("unexpected buffer:\nwant: %q\nhave: %q\n", want, have)
	}
}
