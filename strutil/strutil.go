// Package strutil provides utilities for strings.
package strutil

import (
	"bufio"
	"os"
	"sort"
)

// StringSet provides an unsorted set for strings.
type StringSet map[string]struct{}

// NewStringSet creates and initializes a new StringSet with optional initial values.
func NewStringSet(v ...string) StringSet {
	m := make(StringSet, len(v))
	m.Add(v...)
	return m
}

// Add adds v to s.
func (s StringSet) Add(v ...string) {
	for _, vv := range v {
		s[vv] = struct{}{}
	}
}

// Remove removes v from s.
func (s StringSet) Remove(v ...string) {
	for _, vv := range v {
		delete(s, vv)
	}
}

// Contains returns true when any element of v is present in s.
func (s StringSet) Contains(v ...string) bool {
	for _, vv := range v {
		if _, exists := s[vv]; exists {
			return true
		}
	}
	return false
}

// SortedStrings returns all strings from s, sorted.
func (s StringSet) SortedStrings() []string {
	ss := make([]string, 0, len(s))
	for v := range s {
		ss = append(ss, v)
	}
	sort.Strings(ss)
	return ss
}

// NewStringSetFromFile returns a StringSet from lines of a file.
// Empty lines and lines startwith with '#' are ignored.
func NewStringSetFromFile(name string) (StringSet, error) {
	f, err := os.Open(name)
	if err != nil {
		return nil, err
	}
	defer f.Close()

	s := NewStringSet()
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		t := scanner.Text()
		if len(t) == 0 || t[0] == '#' {
			continue
		}
		s.Add(t)
	}

	return s, scanner.Err()
}

// Unique returns a unique set of values from s.
func Unique(s []string) []string {
	return NewStringSet(s...).SortedStrings()
}

// Merge merges all lists into a set.
func Merge(lists ...[]string) StringSet {
	set := NewStringSet()
	for _, list := range lists {
		set.Add(list...)
	}
	return set
}

// Intersect returns the intersection of all lists as a set.
func Intersect(lists ...[]string) StringSet {
	all := make([]StringSet, len(lists))
	for i := 0; i < len(lists); i++ {
		all[i] = NewStringSet(lists[i]...)
	}

	intersection := NewStringSet()

	for _, listA := range all {
		for k := range listA {
			exists := true

			for _, listB := range all {
				if !listB.Contains(k) {
					exists = false
					break
				}
			}

			if exists {
				intersection.Add(k)
			}
		}
	}

	return intersection
}
