package strutil

import (
	"io/ioutil"
	"os"
	"reflect"
	"testing"
)

func TestStringSet(t *testing.T) {
	s := NewStringSet("foo", "foo", "bar")
	s.Add("baz")
	s.Remove("bar")

	if !s.Contains("foo") {
		t.Fatal("set does not contain item foo")
	}

	have := s.SortedStrings()
	want := []string{"baz", "foo"}

	if !reflect.DeepEqual(have, want) {
		t.Fatalf("unexpected set:\nwant %v\nhave %v\n", want, have)
	}
}

func TestNewStringSetFromFile(t *testing.T) {
	f, err := ioutil.TempFile("", "ush-strutil-")
	if err != nil {
		t.Fatal(err)
	}
	f.WriteString("hello\nworld")
	f.Close()

	defer os.Remove(f.Name())

	s, err := NewStringSetFromFile(f.Name())
	if err != nil {
		t.Fatal(err)
	}

	have := s.SortedStrings()
	want := []string{"hello", "world"}
	if !reflect.DeepEqual(have, want) {
		t.Fatalf("unexpected set:\nwant %v\nhave %v\n", want, have)
	}
}

func TestUnique(t *testing.T) {
	have := Unique([]string{"foo", "foo"})
	want := []string{"foo"}
	if !reflect.DeepEqual(have, want) {
		t.Fatalf("unexpected set:\nwant %v\nhave %v\n", want, have)
	}
}

func TestMerge(t *testing.T) {
	have := Merge([]string{"foo", "bar"}, []string{"foo", "baz"}).SortedStrings()
	want := []string{"bar", "baz", "foo"}
	if !reflect.DeepEqual(have, want) {
		t.Fatalf("unexpected set:\nwant %v\nhave %v\n", want, have)
	}
}

func TestIntersect(t *testing.T) {
	have := Intersect([]string{"foo", "bar"}, []string{"foo", "baz"}).SortedStrings()
	want := []string{"foo"}
	if !reflect.DeepEqual(have, want) {
		t.Fatalf("unexpected set:\nwant %v\nhave %v\n", want, have)
	}
}
