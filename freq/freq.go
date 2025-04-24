// Package freq computes histograms from the results of ush exec.
package freq

import (
	"encoding/json"
	"fmt"
	"io"
	"math"
	"sort"
	"strconv"
	"time"

	"ush/exec"
)

// Item represents a frequency item.
type Item struct {
	Freq    float64  `json:"freq"`
	Value   string   `json:"value"`
	Targets []string `json:"targets"`
}

type resultsReader struct {
	Sorter    func([]Item)
	KeyReader func(*exec.Result) string
}

// Read reads JSON from the output of ush exec. Expects one exec.Result
// document per line from r.
func (r resultsReader) Read(src io.Reader) ([]Item, error) {
	var err error
	var results int

	m := make(map[string][]string)
	dec := json.NewDecoder(src)

	for dec.More() {
		var result exec.Result
		err = dec.Decode(&result)
		if err != nil {
			break
		}

		results++
		k := r.KeyReader(&result)
		targets, exists := m[k]
		if !exists {
			m[k] = []string{result.Target}
		} else {
			m[k] = append(targets, result.Target)
		}
	}

	if len(m) == 0 {
		return nil, err
	}

	items := make([]Item, 0, len(m))
	for text, targets := range m {
		items = append(items, Item{
			Freq:    toFixed(float64(len(targets))*100/float64(results), 2),
			Value:   text,
			Targets: targets,
		})
	}

	r.Sorter(items)
	return items, err
}

func round(num float64) int {
	return int(num + math.Copysign(0.5, num))
}

func toFixed(num float64, precision int) float64 {
	output := math.Pow(10, float64(precision))
	return float64(round(num*output)) / output
}

// Duration returns a set of items grouped and sorted by duration distribution.
func Duration(r io.Reader, truncate time.Duration) ([]Item, error) {
	rr := resultsReader{
		Sorter: func(items []Item) { sort.Sort(itemByDuration(items)) },
		KeyReader: func(result *exec.Result) string {
			d, _ := time.ParseDuration(result.Duration)
			return time.Duration(d.Truncate(truncate) + truncate).String()
		},
	}
	return rr.Read(r)
}

type itemByDuration []Item

func (t itemByDuration) Len() int      { return len(t) }
func (t itemByDuration) Swap(i, j int) { t[i], t[j] = t[j], t[i] }
func (t itemByDuration) Less(i, j int) bool {
	d1, _ := time.ParseDuration(t[i].Value)
	d2, _ := time.ParseDuration(t[j].Value)
	return d1 < d2
}

// ExitStatus returns a set of items grouped and sorted by exit status.
func ExitStatus(r io.Reader) ([]Item, error) {
	rr := resultsReader{
		Sorter:    func(items []Item) { sort.Sort(itemByExitStatus(items)) },
		KeyReader: func(result *exec.Result) string { return strconv.Itoa(result.ExitStatus) },
	}
	return rr.Read(r)
}

type itemByExitStatus []Item

func (t itemByExitStatus) Len() int      { return len(t) }
func (t itemByExitStatus) Swap(i, j int) { t[i], t[j] = t[j], t[i] }
func (t itemByExitStatus) Less(i, j int) bool {
	v1, _ := strconv.Atoi(t[i].Value)
	v2, _ := strconv.Atoi(t[j].Value)
	return v1 < v2
}

// Stderr returns a set of items grouped by stderr, sorted by number of targets.
func Stderr(r io.Reader) ([]Item, error) {
	rr := resultsReader{
		Sorter:    func(items []Item) { sort.Sort(itemByTargets(items)) },
		KeyReader: func(result *exec.Result) string { return result.Stderr },
	}
	return rr.Read(r)
}

// Stdout returns a set of items grouped by stdout, sorted by number of targets.
func Stdout(r io.Reader) ([]Item, error) {
	rr := resultsReader{
		Sorter:    func(items []Item) { sort.Sort(itemByTargets(items)) },
		KeyReader: func(result *exec.Result) string { return result.Stdout },
	}
	return rr.Read(r)
}

type itemByTargets []Item

func (t itemByTargets) Len() int           { return len(t) }
func (t itemByTargets) Swap(i, j int)      { t[i], t[j] = t[j], t[i] }
func (t itemByTargets) Less(i, j int) bool { return len(t[i].Targets) < len(t[j].Targets) }

// EncodeJSON encodes one item per line as JSON to w.
func EncodeJSON(w io.Writer, items []Item) error {
	enc := json.NewEncoder(w)
	for _, item := range items {
		err := enc.Encode(item)
		if err != nil {
			return err
		}
	}
	return nil
}

// EncodeWide encodes a human readable histogram of lines to w.
func EncodeWide(w io.Writer, items []Item) error {
	fmt.Fprintf(w, "%-8s %-8s %-8s %s\n", "count", "targets", "freq %", "value")
	for i, item := range items {
		v := item.Value
		if len(v) > 50 {
			v = v[:50] + "[...]"
		}
		fmt.Fprintf(w, "%-8d %-8d %-6.2f   %q\n", i+1, len(item.Targets), item.Freq, v)
	}
	return nil
}
